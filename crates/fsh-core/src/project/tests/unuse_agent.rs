//! 按 agent 撤回测试：项目与全局两侧共用同一套判定，同一组场景两侧各跑一遍。
//! 名单点满整条记录即整条撤回，是整体撤回的唯一写法。

use std::fs;

use super::*;

/// 两侧都跑一遍：场景与断言完全一致，失败时按侧定位
const SIDES: [Side; 2] = [Side::Project, Side::Global];

#[test]
fn unuse_by_agent_agent_removes_only_its_destination() {
    for side in SIDES {
        let site = Partial::distinct(side);
        let used = site.use_all();
        let only = site.only(0);

        let r = site.unuse(&only, false, false).unwrap();
        assert_eq!(r.targets[0].agents, only, "{side:?} 报告只列撤走的 agent");
        assert_eq!(r.targets[0].removed.len(), 1, "{side:?}");
        assert!(!site.dest(0).exists(), "{side:?}");
        assert!(site.dest(1).exists(), "{side:?} 留下的 agent 落点不得被删");
        assert_eq!(
            site.record_agents().unwrap(),
            vec![site.agents[1].clone()],
            "{side:?}"
        );
        assert_eq!(
            Some(used.targets[0].link),
            r.targets[0].link.map(Some),
            "{side:?} link 沿用登记值"
        );
    }
}

#[test]
fn unuse_by_agent_agent_keeps_a_shared_destination() {
    for side in SIDES {
        let site = Partial::shared(side);
        site.use_all();
        assert!(site.dest(0).exists(), "{side:?} 两个 agent 共用一个落点");

        let r = site.unuse(&site.only(0), false, false).unwrap();
        assert!(
            r.targets[0].removed.is_empty(),
            "{side:?} 还有 agent 在用时不得删落点"
        );
        assert!(site.dest(0).exists(), "{side:?} 留下那一方的入口不得断");
        assert_eq!(
            site.record_agents().unwrap(),
            vec![site.agents[1].clone()],
            "{side:?}"
        );
    }
}

#[test]
fn unuse_by_agent_last_holder_removes_the_shared_destination() {
    for side in SIDES {
        let site = Partial::shared(side);
        site.use_all();
        site.unuse(&site.only(0), false, false).unwrap();

        // 最后一个引用它的 agent 也撤走：目录与记录一起没了
        let r = site.unuse(&site.only(1), false, false).unwrap();
        assert_eq!(r.targets[0].removed.len(), 1, "{side:?}");
        assert!(!site.dest(0).exists(), "{side:?}");
        assert!(site.record_agents().is_none(), "{side:?}");
        assert!(site.records_empty(), "{side:?}");
    }
}

#[test]
fn unuse_by_agent_listing_every_agent_matches_whole_withdrawal() {
    for side in SIDES {
        let site = Partial::distinct(side);
        site.use_all();
        let all = site.agents.clone();

        // 名单点满整条记录：落点全删、记录与项目身份一起清掉
        let r = site.unuse(&all, false, false).unwrap();
        assert_eq!(r.targets[0].agents, all, "{side:?}");
        assert_eq!(r.targets[0].removed.len(), 2, "{side:?}");
        assert!(site.record_agents().is_none(), "{side:?}");
        assert!(site.records_empty(), "{side:?}");
    }
}

#[test]
fn unuse_rejects_an_agent_that_the_record_does_not_have() {
    for side in SIDES {
        let site = Partial::distinct(side);
        site.use_all();
        let nobody = ["nobody".to_string()];

        let err = site.unuse(&nobody, false, false).unwrap_err();
        assert_eq!(err.error_code(), "agent_not_registered", "{side:?}");
        // 名字写错时不猜：错误点名该 agent，并给出记录已登记的清单
        let message = err.to_string();
        assert!(
            message.contains("nobody"),
            "{side:?} 错误要点名 agent: {message}"
        );
        assert!(
            message.contains(&site.agents[0]),
            "{side:?} 错误要给出已登记清单: {message}"
        );
        assert!(site.dest(0).exists(), "{side:?}");
        assert_eq!(site.record_agents().unwrap(), site.agents, "{side:?}");
    }
}

#[test]
fn unuse_by_agent_rejects_a_keeping_agent_that_no_longer_resolves() {
    for side in SIDES {
        let site = Partial::shared(side);
        site.use_all();
        site.remove_agent_config(1);

        // 保留那一方的配置没了：判不了共用关系就整次不动盘
        let err = site.unuse(&site.only(0), false, false).unwrap_err();
        assert_eq!(err.error_code(), "unknown_agent", "{side:?}");
        assert!(site.dest(0).exists(), "{side:?}");
        assert_eq!(site.record_agents().unwrap(), site.agents, "{side:?}");
    }
}

#[test]
fn unuse_by_agent_rejects_a_keeping_agent_without_the_scope_dir() {
    for side in SIDES {
        let site = Partial::shared(side);
        site.use_all();
        site.move_agent_to_other_side(1);

        // 保留方在这个作用域没有目录：同样整次不动盘
        let err = site.unuse(&site.only(0), false, false).unwrap_err();
        assert_eq!(err.error_code(), site.no_side_error(), "{side:?}");
        assert!(site.dest(0).exists(), "{side:?}");
        assert_eq!(site.record_agents().unwrap(), site.agents, "{side:?}");
    }
}

#[test]
fn unuse_by_agent_skips_a_missing_destination_but_still_drops_the_agent() {
    for side in SIDES {
        let site = Partial::distinct(side);
        site.use_all();
        fs::remove_dir_all(site.dest(0)).unwrap();

        // 落点已缺失 = 已撤回：报告不列它，但该 agent 照样从记录里摘掉
        let r = site.unuse(&site.only(0), false, false).unwrap();
        assert!(r.targets[0].removed.is_empty(), "{side:?}");
        assert_eq!(
            site.record_agents().unwrap(),
            vec![site.agents[1].clone()],
            "{side:?}"
        );
    }
}

#[test]
fn unuse_by_agent_dry_run_lists_only_the_withdrawn_destination() {
    for side in SIDES {
        let site = Partial::distinct(side);
        site.use_all();

        let actions = site.unuse(&site.only(0), false, true).unwrap().targets[0]
            .actions
            .clone();
        assert_eq!(actions.len(), 1, "{side:?} 只列将删的那一处: {actions:?}");
        assert!(site.dest(0).exists(), "{side:?} 预演不得删");
        assert_eq!(
            site.record_agents().unwrap(),
            site.agents,
            "{side:?} 预演不写登记簿"
        );
    }
}

#[test]
fn unuse_with_an_empty_agent_list_is_a_no_op() {
    for side in SIDES {
        let site = Partial::distinct(side);
        site.use_all();

        // 空名单是空撤回：没有要撤的 agent，落点与登记都不动
        let r = site.unuse(&[], false, false).unwrap();
        assert!(r.targets[0].agents.is_empty(), "{side:?}");
        assert!(r.targets[0].removed.is_empty(), "{side:?}");
        assert!(site.dest(0).exists() && site.dest(1).exists(), "{side:?}");
        assert_eq!(site.record_agents().unwrap(), site.agents, "{side:?}");
    }
}

#[test]
fn unuse_by_agent_protects_a_modified_copy_unless_forced() {
    for side in SIDES {
        let site = Partial::distinct(side);
        let used = site.use_all();
        // 档位取决于本机三级降级结果：copy 档断言修改保护，链接档断言被替换后始终拒绝
        if used.targets[0].link == Some(LinkKind::Copy) {
            fs::write(site.dest(0).join("SKILL.md"), "手改内容").unwrap();
            let err = site.unuse(&site.only(0), false, false).unwrap_err();
            assert_eq!(err.error_code(), "modified_copy", "{side:?}");
            assert!(site.dest(0).is_dir(), "{side:?} 拒绝时不得删除");
            site.unuse(&site.only(0), true, false).unwrap();
            assert!(!site.dest(0).exists(), "{side:?}");
            assert!(site.dest(1).exists(), "{side:?} 保留方的落点不受影响");
        } else {
            fs::remove_dir(site.dest(0)).unwrap();
            fs::create_dir_all(site.dest(0)).unwrap();
            fs::write(site.dest(0).join("user.txt"), "keep").unwrap();
            for force in [false, true] {
                let err = site.unuse(&site.only(0), force, false).unwrap_err();
                assert_eq!(err.error_code(), "link_kind_mismatch", "{side:?}");
                assert!(
                    site.dest(0).join("user.txt").is_file(),
                    "{side:?} force 也不得覆盖"
                );
            }
        }
    }
}
