//! 机器提交消息：写入口只说「我做了什么」，本模块把动词、目标面与对象
//! 拼成 `<动词>(<目标>): <对象>` 形态的提交消息文本。
//! 动词与目标面各是一处词表，调用方只登记词，不写格式串；对象只取键与
//! 固定文案这类相对标识，禁止绝对路径。

use std::fmt;

/// 机器提交的动词词表：改到 hub 跟踪面的写入口各登记一个动词
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verb {
    /// 入库
    Add,
    /// 归档
    Archive,
    /// 恢复
    Restore,
    /// 彻底删除
    Remove,
    /// 初始化
    Init,
    /// 补齐半截装配
    Repair,
    /// 键迁移
    Move,
    /// 手改快照
    Sync,
}

impl Verb {
    /// 动词在消息里的写法
    fn word(self) -> &'static str {
        match self {
            Verb::Add => "add",
            Verb::Archive => "archive",
            Verb::Restore => "restore",
            Verb::Remove => "remove",
            Verb::Init => "init",
            Verb::Repair => "repair",
            Verb::Move => "move",
            Verb::Sync => "sync",
        }
    }
}

/// 库管理面的目标面词表：消息里的目标取固定面名
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Face {
    /// 工作副本面 `store\`
    Store,
    /// 归档面 `archive\`
    Archive,
    /// hub 根装配
    Hub,
    /// 内置技能面 `store\builtin\`
    Builtin,
}

impl Face {
    /// 目标面在消息里的写法
    fn word(self) -> &'static str {
        match self {
            Face::Store => "store",
            Face::Archive => "archive",
            Face::Hub => "hub",
            Face::Builtin => "builtin",
        }
    }
}

/// 机器提交的目标：库管理面的固定面，或一次操作触及的多个面
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target<'a> {
    /// 库管理面
    Face(Face),
    /// 库管理面一次操作触及的多个面：按给定顺序用逗号连接
    Faces(&'a [Face]),
}

impl fmt::Display for Target<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Target::Face(face) => f.write_str(face.word()),
            Target::Faces(faces) => f.write_str(
                &faces
                    .iter()
                    .map(|face| face.word())
                    .collect::<Vec<_>>()
                    .join(","),
            ),
        }
    }
}

/// 库管理面的一条机器提交：目标面取固定词表，对象是相对标识
pub fn message(verb: Verb, face: Face, object: &str) -> String {
    render(verb, &Target::Face(face), object)
}

/// 库管理面一次操作触及多个面时的机器提交：目标面按给定顺序用逗号连接
pub fn message_faces(verb: Verb, faces: &[Face], object: &str) -> String {
    render(verb, &Target::Faces(faces), object)
}

/// 把动词、目标与对象拼成消息形态 `<动词>(<目标>): <对象>`
fn render(verb: Verb, target: &Target<'_>, object: &str) -> String {
    format!("{}({}): {}", verb.word(), target, object)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 同一键在 store 面与 archive 面：目标面不同，消息因此不同
    #[test]
    fn same_key_on_two_faces_differs_by_target() {
        let store = message(Verb::Remove, Face::Store, "dead");
        let archive = message(Verb::Remove, Face::Archive, "dead");
        assert_eq!(store, "remove(store): dead");
        assert_eq!(archive, "remove(archive): dead");
        assert_ne!(store, archive, "同一键在两面产出的消息必须不同");
    }

    /// 一次操作触及多个面：目标面按给定顺序用逗号连接
    #[test]
    fn multiple_faces_join_in_given_order() {
        assert_eq!(
            message_faces(Verb::Remove, &[Face::Store, Face::Archive], "dead"),
            "remove(store,archive): dead"
        );
        assert_eq!(
            message_faces(Verb::Remove, &[Face::Archive], "dead"),
            "remove(archive): dead",
            "单面与固定面写法一致"
        );
    }
}
