//! 盘面原语：写骨架落盘时的全部动盘动作。
//! 每个原语在动手那一刻自己登记撤销，入口没有别的地方可登记；
//! 撤销清单因而只反映真做过的动作，落盘失败时按登记的逆序一次撤干净。

use std::collections::BTreeMap;
use std::path::Path;

use crate::error::CommonError;
use crate::git;
use crate::hub;
use crate::link::{self, LinkError, LinkKind};
use crate::lock::WriteToken;
use crate::paths::HubPaths;
use crate::plan::{Act, Step};

use super::OnFailure;

/// 一次实跑的落盘现场：写锁、路径族与本次已做动作的撤销清单
pub(crate) struct Ctx<'a> {
    /// 全局写锁凭证：登记簿写回只认它
    token: &'a WriteToken,
    /// hub 路径族
    paths: &'a HubPaths,
    /// 失败时的盘面处置
    failure: OnFailure,
    /// 本次已做盘上动作的撤销清单，按登记的逆序执行
    undo: Vec<Box<dyn FnOnce()>>,
    /// 本入口的仓库句柄：装配入口在建出仓库之前就开工，故由落盘段交回
    pub(super) repo: Option<git::Repository>,
    /// 登记簿缓存：一次落盘只读一次，写回成功后随之更新
    registry: Option<hub::ProjectRegistry>,
    /// 本次落盘逐组定下的档位：组内落点共用它，登记记录也照它写
    landed: BTreeMap<usize, LinkKind>,
}

impl<'a> Ctx<'a> {
    /// 落盘现场：锁凭证与路径族已定，仓库句柄由开仓或落盘段提供
    pub(super) fn new(token: &'a WriteToken, paths: &'a HubPaths, failure: OnFailure) -> Self {
        Self {
            token,
            paths,
            failure,
            undo: Vec::new(),
            repo: None,
            registry: None,
            landed: BTreeMap::new(),
        }
    }

    /// hub 路径族：只读方取用；动盘一律走本现场的动作
    pub(crate) fn paths(&self) -> &HubPaths {
        self.paths
    }

    /// 交回仓库句柄：首建的装配入口在落盘段里才把仓库建出来
    pub(crate) fn attach_repo(&mut self, repo: git::Repository) {
        self.repo = Some(repo);
    }

    /// 按计划顺序执行盘面动作：一步一条，撤销由各原语就地登记
    pub(crate) fn land<E>(&mut self, steps: &[Step]) -> Result<(), E>
    where
        E: From<std::io::Error> + From<LinkError>,
    {
        for step in steps {
            match &step.act {
                Act::None => {}
                Act::Move { from, to } => self.move_dir(from, to)?,
                Act::Copy { from, to } => self.copy_dir(from, to)?,
                Act::Remove(dir) => self.remove_dir(dir)?,
                Act::Link { from, to, group } => {
                    self.link_dir(from, to, *group)?;
                }
                Act::Unlink { path, kind } => self.unlink(path, *kind)?,
                Act::Relink {
                    path,
                    from,
                    to,
                    kind,
                } => self.relink(path, *kind, from, to)?,
            }
        }
        Ok(())
    }

    /// 本次落盘给这一组落点定下的档位：没建过就是没有
    pub(crate) fn group_kind(&self, group: usize) -> Option<LinkKind> {
        self.landed.get(&group).copied()
    }

    /// 登记一条撤销动作：落盘失败且策略为回滚时由骨架逆序执行。
    /// 撤销动作自己负责在删不掉时留痕，骨架只管按序调用。
    /// 只给本现场的原语用——盘面动作一律经原语做，登记点因此不会漏
    fn on_rollback(&mut self, undo: impl FnOnce() + 'static) {
        self.undo.push(Box::new(undo));
    }

    /// 挪目录：父层目录建出 → 改名 → 就地登记「改名回来」。
    /// 改名没成就没有可撤的动作，故只在改名成功后登记
    pub(crate) fn move_dir(&mut self, from: &Path, to: &Path) -> std::io::Result<()> {
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::rename(from, to)?;
        let (back_to, moved_from) = (from.to_path_buf(), to.to_path_buf());
        self.on_rollback(move || {
            if let Err(e) = std::fs::rename(&moved_from, &back_to) {
                tracing::warn!(path = %moved_from.display(), cause = %e, "回滚目录挪动失败");
            }
        });
        Ok(())
    }

    /// 复制目录树（任何深度的 `.git` 都不带入）：先建立点并登记撤销，
    /// 再逐项拷贝——拷到一半失败时，已写下的半棵树同样要删净
    pub(crate) fn copy_dir(&mut self, from: &Path, to: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(to)?;
        let undo = to.to_path_buf();
        self.on_rollback(move || {
            if undo.exists()
                && let Err(e) = std::fs::remove_dir_all(&undo)
            {
                tracing::warn!(path = %undo.display(), cause = %e, "回滚复制的目录删除失败");
            }
        });
        copy_tree(from, to)?;
        Ok(())
    }

    /// 删目录：删除不可逆，已删的部分保持删掉，配 [`OnFailure::Resume`] 重跑续完
    pub(crate) fn remove_dir(&mut self, dir: &Path) -> std::io::Result<()> {
        if dir.exists() {
            std::fs::remove_dir_all(dir)?;
        }
        Ok(())
    }

    /// 建目录：空目录不进提交，重跑无副作用，故不登记撤销
    pub(crate) fn create_dir(&mut self, dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dir)?;
        Ok(())
    }

    /// 写文件：父层目录建出 → 登记「写回原内容 / 删掉新建的」→ 写。
    /// 先登记再写，写到一半失败也能把原内容摆回去
    pub(crate) fn write_file(&mut self, path: &Path, body: &[u8]) -> std::io::Result<()> {
        let previous = std::fs::read(path).ok();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let undo_path = path.to_path_buf();
        self.on_rollback(move || match previous {
            Some(bytes) => {
                if let Err(e) = std::fs::write(&undo_path, bytes) {
                    tracing::warn!(path = %undo_path.display(), cause = %e, "回滚文件写回失败");
                }
            }
            None => {
                if let Err(e) = std::fs::remove_file(&undo_path)
                    && e.kind() != std::io::ErrorKind::NotFound
                {
                    tracing::warn!(path = %undo_path.display(), cause = %e, "回滚新建文件删除失败");
                }
            }
        });
        std::fs::write(path, body)?;
        Ok(())
    }

    /// 建落点：建出指向 `from` 的入口并就地登记「删掉它」。
    /// 同一组的落点共用一个档位：组里第一个按本机降级链定档并记下，
    /// 其余按这一定下的档位建，办不到即整笔失败
    pub(crate) fn link_dir(
        &mut self,
        from: &Path,
        to: &Path,
        group: usize,
    ) -> Result<LinkKind, LinkError> {
        let built = match self.landed.get(&group) {
            Some(kind) => link::create_dir_link_pinned(from, to, *kind).map(|()| *kind),
            None => link::create_dir_link(from, to),
        };
        let kind = match built {
            Ok(kind) => kind,
            Err(e) => {
                // 建链接失败是本次使用的失败边界：记下哪个落点失败与原因，随后整批回滚
                tracing::warn!(path = %to.display(), cause = %e, "落点建链接失败");
                return Err(e);
            }
        };
        let undo = to.to_path_buf();
        self.on_rollback(move || {
            // 回滚删不掉会留下半套入口，是唯一能发现残留的地方，必须留痕
            if let Err(e) = link::remove_link(&undo, kind) {
                tracing::warn!(path = %undo.display(), cause = %e, "回滚落点删除失败");
            }
        });
        self.landed.entry(group).or_insert(kind);
        Ok(kind)
    }

    /// 撤落点：按登记的档位删，目标不在场视为已撤销（幂等）。
    /// 删除不可逆，配 [`OnFailure::Resume`] 重跑续完
    pub(crate) fn unlink(&mut self, path: &Path, kind: LinkKind) -> Result<(), LinkError> {
        if let Err(e) = link::remove_link(path, kind) {
            tracing::warn!(path = %path.display(), cause = %e, "落点删除失败");
            return Err(e);
        }
        Ok(())
    }

    /// 换源重链：删掉 `path` 上的旧入口，按登记的档位重建指向 `to` 的入口。
    /// 档位办不到即整笔失败，不静默降级——降级会让登记与盘上从此各说一套。
    /// 撤销先登记再建：删新链、按 `from` 以同一档位重建旧链
    pub(crate) fn relink(
        &mut self,
        path: &Path,
        kind: LinkKind,
        from: &Path,
        to: &Path,
    ) -> Result<(), LinkError> {
        link::remove_link(path, kind)?;
        let (undo_path, old_source) = (path.to_path_buf(), from.to_path_buf());
        self.on_rollback(move || {
            if let Err(e) = link::remove_link(&undo_path, kind) {
                tracing::warn!(path = %undo_path.display(), cause = %e, "回滚重链落点删除失败");
                return;
            }
            if let Err(e) = link::create_dir_link_pinned(&old_source, &undo_path, kind) {
                tracing::warn!(path = %undo_path.display(), cause = %e, "回滚重链落点重建失败");
            }
        });
        link::create_dir_link_pinned(to, path, kind)?;
        Ok(())
    }

    /// 改登记簿：读一次（缓存）→ 改 → 原子写回。登记簿只有这一个写入口，
    /// 写回失败时缓存不更新，盘上与缓存都不留半套改动
    pub(crate) fn edit_registry<E>(
        &mut self,
        mutate: impl FnOnce(&mut hub::ProjectRegistry) -> Result<(), E>,
    ) -> Result<(), E>
    where
        E: From<CommonError>,
    {
        let mut registry = match self.registry.take() {
            Some(cached) => cached,
            None => hub::load_registry(self.paths).map_err(CommonError::from)?,
        };
        mutate(&mut registry)?;
        hub::save_registry(self.paths, &registry, self.token).map_err(CommonError::from)?;
        self.registry = Some(registry);
        Ok(())
    }

    /// 失败处置：回滚策略下逆序执行全部撤销动作，续跑策略下只丢弃清单。
    /// 清单随现场析构而清空，不留跨次操作的状态
    pub(super) fn settle(&mut self) {
        if self.failure == OnFailure::Rollback {
            for undo in self.undo.drain(..).rev() {
                undo();
            }
        }
    }
}

/// 递归复制目录树，跳过任何深度的 .git 目录（源自身是仓库时不带入嵌套仓库）
fn copy_tree(src: &Path, dst: &Path) -> std::io::Result<()> {
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let name = entry.file_name();
        if name == ".git" {
            continue;
        }
        let from = entry.path();
        let to = dst.join(&name);
        if from.is_dir() {
            std::fs::create_dir_all(&to)?;
            copy_tree(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)?;
        }
    }
    Ok(())
}
