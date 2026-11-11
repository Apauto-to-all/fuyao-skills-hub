//! 键入参形状：收 `<skill>` 的每个公开入口都先过 key 模块，
//! 写错分隔符或踩段内规则要当场点名，不落到「技能不存在」。

use super::*;

#[test]
fn skill_entry_points_reject_malformed_key() {
    let (install, _paths) = setup();
    let hub = root(&install);
    for bad in ["mattpocock\\skills\\tdd", "C:/x"] {
        let checks: [(&str, Result<(), SkillError>); 3] = [
            ("archive", archive(&hub, bad, false).map(|_| ())),
            ("restore", restore(&hub, bad, false).map(|_| ())),
            ("remove", remove(&hub, bad, false).map(|_| ())),
        ];
        for (name, result) in checks {
            assert_eq!(
                result.unwrap_err().error_code(),
                "invalid_skill_key",
                "{name} 应先报键写法错误: {bad}"
            );
        }
    }
}
