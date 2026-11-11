//! 内容指纹：给盘上目录算一把内容哈希，供落点内容等值比对。
//! 本模块是叶子：只依赖文件系统，不认识 hub、项目或回收策略。

mod dir;

pub(crate) use dir::dir_digest;

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
