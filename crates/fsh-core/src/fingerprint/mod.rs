//! 内容指纹：给「盘上目录」与「git tag 子树」各算一把同口径的哈希。
//! 两把哈希必须同口径，唯一的理由是拿盘上目录和 tag 子树做等值比对。
//! 本模块是叶子：只依赖文件系统与 git 对象库，不认识 hub、项目或回收策略。

use crate::naming;

mod dir;
mod tag;

pub(crate) use dir::dir_digest;

/// 以技能键定位 tag 子树的指纹，供只持有完整 tag 名的调用方使用
pub(crate) fn tag_digest(repo: &git2::Repository, tag: &str, key: &str) -> Option<String> {
    tag::tag_digest_at(repo, tag, &naming::store_subpath(key))
}

/// 盘上目录指纹；读不了时给 `None`，让调用方按「不匹配」处理
pub(crate) fn dir_digest_best_effort(dir: &std::path::Path) -> Option<String> {
    dir_digest(dir).ok()
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    /// 按给定名字建目录并写文件
    pub(super) fn build(dir: &Path, files: &[(&str, &str)]) {
        for (rel, body) in files {
            let p = dir.join(rel);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, body).unwrap();
        }
    }
}
