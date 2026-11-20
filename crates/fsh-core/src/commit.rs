//! 机器提交消息：写入口只说「我做了什么」，本模块把动词、目标面与对象
//! 拼成 `<动词>(<目标>): <对象>` 形态的提交消息文本。
//! 动词与目标面各是一处词表，调用方只登记词，不写格式串；对象只取键与
//! 固定文案这类相对标识，禁止绝对路径。

use std::fmt;

use crate::select::ROOT_LABEL;

/// 写入口的动词词表：改到 hub 跟踪面的入口各登记一个动词；
/// 不动跟踪面的入口也在这里登记，只给完成陈述取名
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
    /// 铺装（落点在项目与 agent 目录里，不产生提交）
    Use,
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
            Verb::Use => "use",
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

/// 库管理面一次操作触及多个面时的机器提交：目标面按给定顺序用逗号连接
pub fn batch_message(verb: Verb, faces: &[Face], keys: &[String]) -> String {
    message_faces(verb, faces, &object(keys))
}

/// 提交对象按实际动到的键决定：只动一个键就点那个键，两个及以上才写数量。
/// 判定只住这里，动词侧只给面与键清单
fn object(keys: &[String]) -> String {
    match keys {
        [] => String::new(),
        [key] => key.clone(),
        keys => format!("{} {} 个技能", set_prefix(keys), keys.len()),
    }
}

/// 一套技能的前缀：各键的共同父路径，一段都不共同即「根」。
/// 前缀省略时整套就落在根下，浅的键与深的键混在一套里也只报到共同那一层
fn set_prefix(keys: &[String]) -> String {
    // 第一个键的父路径作候选，其余键逐段砍到共同的部分
    let mut prefix: Vec<&str> = keys[0].split('/').collect();
    prefix.pop();
    for key in &keys[1..] {
        let segments: Vec<&str> = key.split('/').collect();
        let shared = prefix
            .iter()
            .zip(&segments)
            .take_while(|(mine, other)| mine == other)
            .count();
        prefix.truncate(shared);
    }
    if prefix.is_empty() {
        ROOT_LABEL.to_string()
    } else {
        prefix.join("/")
    }
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

    /// 一次只动一个键：消息点那个键本身，比「整套 + 条数」更具体
    #[test]
    fn one_key_is_pointed_by_name() {
        assert_eq!(
            batch_message(Verb::Remove, &[Face::Store], &["dead".to_string()]),
            "remove(store): dead"
        );
        assert_eq!(
            batch_message(Verb::Archive, &[Face::Store], &["k1/k2".to_string()]),
            "archive(store): k1/k2",
            "点的是整键，前缀原样带着"
        );
    }

    /// 两个及以上才写数量：前缀取这些键的共同父路径，一条提交说清动了哪一套、几条
    #[test]
    fn several_keys_write_the_set_prefix_and_the_count() {
        assert_eq!(
            batch_message(
                Verb::Archive,
                &[Face::Store],
                &[
                    "k1/k2/a".to_string(),
                    "k1/k2/b".to_string(),
                    "k1/k2/c".to_string()
                ]
            ),
            "archive(store): k1/k2 3 个技能"
        );
        assert_eq!(
            batch_message(
                Verb::Archive,
                &[Face::Store],
                &["a".to_string(), "b".to_string()]
            ),
            "archive(store): 根 2 个技能",
            "前缀省略即根"
        );
        assert_eq!(
            batch_message(
                Verb::Remove,
                &[Face::Store],
                &["k1/a".to_string(), "k2/b".to_string()]
            ),
            "remove(store): 根 2 个技能",
            "跨前缀的一套落回根"
        );
    }

    /// 一次操作触及多个面：目标面按给定顺序用逗号连接，单键时对象仍是那个键
    #[test]
    fn batch_message_names_every_touched_face() {
        assert_eq!(
            batch_message(
                Verb::Remove,
                &[Face::Store, Face::Archive],
                &["k1/k2/a".to_string(), "k1/k2/b".to_string()]
            ),
            "remove(store,archive): k1/k2 2 个技能"
        );
        assert_eq!(
            batch_message(
                Verb::Remove,
                &[Face::Store, Face::Archive],
                &["dead".to_string()]
            ),
            "remove(store,archive): dead",
            "两个面同删一个键时点键"
        );
        assert_eq!(
            batch_message(Verb::Remove, &[Face::Archive], &["k1/k2".to_string()]),
            "remove(archive): k1/k2",
            "单面与多面写法一致"
        );
    }
}
