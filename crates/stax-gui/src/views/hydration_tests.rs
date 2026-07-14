use super::*;

#[gpui::test]
fn initial_workspace_hydration_dispatches_details_diff_and_ci(cx: &mut TestAppContext) {
    let hydration = Arc::new(FixtureHydration::new(
        |_, _| Ok(details(3, true)),
        |_, _, _| Ok(None),
        |_, _, _| Ok(diff("fresh patch")),
        |_, _| Ok(ci("success")),
    ));
    let services = services_with_hydration(
        Ok(snapshot("/repo")),
        Ok(None),
        Arc::new(FixtureRecents::default()),
        hydration.clone(),
    );
    let (view, cx) = cx.add_window_view(|window, cx| {
        AppView::new(Some(PathBuf::from("/repo")), services, window, cx)
    });
    cx.run_until_parked();

    let calls = hydration.calls();
    assert!(calls.contains(&HydrationCall::Details {
        repository: PathBuf::from("/repo"),
        branch: "feature-a".into(),
    }));
    assert!(calls.contains(&HydrationCall::CachedDiff {
        branch: "feature-a".into(),
        parent: "main".into(),
    }));
    assert!(calls.contains(&HydrationCall::Diff {
        branch: "feature-a".into(),
        parent: "main".into(),
    }));
    assert!(calls.contains(&HydrationCall::Ci {
        repository: PathBuf::from("/repo"),
        branch: "feature-a".into(),
    }));
    cx.update(|_, app| {
        let state = view.read(app).workspace().unwrap().state();
        assert_eq!(state.details().ready(), Some(&details(3, true)));
        assert_eq!(state.diff().ready(), Some(&diff("fresh patch")));
        assert_eq!(state.ci().ready(), Some(&ci("success")));
    });
}

#[gpui::test]
fn blocked_details_does_not_prevent_diff_completion(cx: &mut TestAppContext) {
    let details_gate = Arc::new(Gate::default());
    let diff_completed = Arc::new(AtomicBool::new(false));
    let details_gate_for_service = Arc::clone(&details_gate);
    let diff_completed_for_service = Arc::clone(&diff_completed);
    let hydration = Arc::new(FixtureHydration::new_async(
        move |_, _| {
            let details_gate = Arc::clone(&details_gate_for_service);
            Box::pin(async move {
                details_gate.wait().await;
                Ok(details(1, false))
            })
        },
        |_, _, _| Box::pin(async { Ok(None) }),
        move |_, _, _| {
            let diff_completed = Arc::clone(&diff_completed_for_service);
            Box::pin(async move {
                diff_completed.store(true, Ordering::SeqCst);
                Ok(diff("overlapping patch"))
            })
        },
        |_, _| Box::pin(async { Ok(ci("success")) }),
    ));
    let services = services_with_hydration(
        Ok(snapshot("/repo")),
        Ok(None),
        Arc::new(FixtureRecents::default()),
        hydration,
    );
    let details_gate_for_release = Arc::clone(&details_gate);
    let diff_completed_for_release = Arc::clone(&diff_completed);
    let overlap_observed = Arc::new(AtomicBool::new(false));
    let overlap_observed_for_release = Arc::clone(&overlap_observed);
    let release = thread::spawn(move || {
        assert!(details_gate_for_release.wait_until_started(Duration::from_secs(5)));
        let deadline = Instant::now() + Duration::from_secs(5);
        while !diff_completed_for_release.load(Ordering::SeqCst) && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }
        overlap_observed_for_release.store(
            diff_completed_for_release.load(Ordering::SeqCst),
            Ordering::SeqCst,
        );
        details_gate_for_release.release();
    });

    let (view, cx) = cx.add_window_view(|window, cx| {
        AppView::new(Some(PathBuf::from("/repo")), services, window, cx)
    });
    cx.run_until_parked();
    release.join().unwrap();
    cx.run_until_parked();

    assert!(overlap_observed.load(Ordering::SeqCst));
    cx.update(|_, app| {
        let state = view.read(app).workspace().unwrap().state();
        assert_eq!(state.diff().ready(), Some(&diff("overlapping patch")));
        assert_eq!(state.details().ready(), Some(&details(1, false)));
    });
}

#[gpui::test]
fn no_remote_skips_ci_service_and_shows_push_guidance(cx: &mut TestAppContext) {
    let hydration = Arc::new(FixtureHydration::immediate_no_remote());
    let services = services_with_hydration(
        Ok(snapshot("/repo")),
        Ok(None),
        Arc::new(FixtureRecents::default()),
        hydration.clone(),
    );
    let (view, cx) = cx.add_window_view(|window, cx| {
        AppView::new(Some(PathBuf::from("/repo")), services, window, cx)
    });
    cx.run_until_parked();

    assert!(
        !hydration
            .calls()
            .iter()
            .any(|call| matches!(call, HydrationCall::Ci { .. }))
    );
    cx.update(|_, app| {
        let state = view.read(app).workspace().unwrap().state();
        assert!(
            state
                .ci()
                .error()
                .is_some_and(|error| error.contains("Push branch"))
        );
        assert!(state.details().ready().is_some());
        assert!(state.diff().ready().is_some());
    });
}

#[gpui::test]
fn details_failure_does_not_discard_an_independent_diff_success(cx: &mut TestAppContext) {
    let hydration = Arc::new(FixtureHydration::new(
        |_, _| Err("details unavailable; verify repository config".into()),
        |_, _, _| Ok(None),
        |_, _, _| Ok(diff("independent patch")),
        |_, _| panic!("CI must not run when details fail"),
    ));
    let services = services_with_hydration(
        Ok(snapshot("/repo")),
        Ok(None),
        Arc::new(FixtureRecents::default()),
        hydration,
    );
    let (view, cx) = cx.add_window_view(|window, cx| {
        AppView::new(Some(PathBuf::from("/repo")), services, window, cx)
    });
    cx.run_until_parked();

    cx.update(|_, app| {
        let state = view.read(app).workspace().unwrap().state();
        assert_eq!(
            state.details().error(),
            Some("details unavailable; verify repository config")
        );
        assert_eq!(state.diff().ready(), Some(&diff("independent patch")));
        assert!(
            state
                .ci()
                .error()
                .is_some_and(|error| error.contains("details"))
        );
    });
}

#[gpui::test]
fn diff_failure_does_not_block_details_or_ci_success(cx: &mut TestAppContext) {
    let hydration = Arc::new(FixtureHydration::new(
        |_, _| Ok(details(2, true)),
        |_, _, _| Ok(None),
        |_, _, _| Err("diff refs changed; refresh again".into()),
        |_, _| Ok(ci("success")),
    ));
    let services = services_with_hydration(
        Ok(snapshot("/repo")),
        Ok(None),
        Arc::new(FixtureRecents::default()),
        hydration,
    );
    let (view, cx) = cx.add_window_view(|window, cx| {
        AppView::new(Some(PathBuf::from("/repo")), services, window, cx)
    });
    cx.run_until_parked();

    cx.update(|_, app| {
        let state = view.read(app).workspace().unwrap().state();
        assert_eq!(state.details().ready(), Some(&details(2, true)));
        assert_eq!(
            state.diff().error(),
            Some("diff refs changed; refresh again")
        );
        assert_eq!(state.ci().ready(), Some(&ci("success")));
    });
}

#[gpui::test]
fn trunk_hydration_uses_an_immediate_empty_diff_without_diff_service_calls(
    cx: &mut TestAppContext,
) {
    let trunk_snapshot = RepositorySnapshot {
        repository_root: PathBuf::from("/repo"),
        current_branch: "main".into(),
        trunk: "main".into(),
        branches: vec![branch("main", None, true, true)],
    };
    let hydration = Arc::new(FixtureHydration::immediate_no_remote());
    let services = services_with_hydration(
        Ok(trunk_snapshot),
        Ok(None),
        Arc::new(FixtureRecents::default()),
        hydration.clone(),
    );
    let (view, cx) = cx.add_window_view(|window, cx| {
        AppView::new(Some(PathBuf::from("/repo")), services, window, cx)
    });
    cx.run_until_parked();

    assert!(!hydration.calls().iter().any(|call| matches!(
        call,
        HydrationCall::CachedDiff { .. } | HydrationCall::Diff { .. }
    )));
    cx.update(|_, app| {
        let state = view.read(app).workspace().unwrap().state();
        assert_eq!(
            state.diff(),
            &LoadState::Ready(BranchDiff {
                stat: Vec::new(),
                lines: Vec::new(),
            })
        );
        assert!(!state.diff_is_refreshing());
    });
}

#[gpui::test]
fn rapid_branch_switch_rejects_late_details_and_diff_before_ci_dispatch(cx: &mut TestAppContext) {
    let details_gate = Arc::new(Gate::default());
    let diff_gate = Arc::new(Gate::default());
    let details_gate_for_service = Arc::clone(&details_gate);
    let diff_gate_for_service = Arc::clone(&diff_gate);
    let hydration = Arc::new(FixtureHydration::new_async(
        move |_, branch| {
            let details_gate = Arc::clone(&details_gate_for_service);
            Box::pin(async move {
                if branch.name == "feature-a" {
                    details_gate.wait().await;
                    Ok(details(99, true))
                } else {
                    Ok(details(2, true))
                }
            })
        },
        |_, _, _| Box::pin(async { Ok(None) }),
        move |_, branch, _| {
            let diff_gate = Arc::clone(&diff_gate_for_service);
            Box::pin(async move {
                if branch == "feature-a" {
                    diff_gate.wait().await;
                    Ok(diff("stale feature-a patch"))
                } else {
                    Ok(diff("feature-b patch"))
                }
            })
        },
        |_, branch| {
            Box::pin(async move {
                assert_ne!(branch, "feature-a", "stale details must not dispatch CI");
                Ok(ci("success"))
            })
        },
    ));
    let services = services_with_hydration(
        Ok(snapshot("/repo")),
        Ok(None),
        Arc::new(FixtureRecents::default()),
        hydration.clone(),
    );
    let (view, cx) = cx.add_window_view(|window, cx| {
        AppView::new(Some(PathBuf::from("/repo")), services, window, cx)
    });
    assert!(details_gate.wait_until_started(Duration::from_secs(5)));
    assert!(diff_gate.wait_until_started(Duration::from_secs(5)));

    view.update_in(cx, |view, window, cx| {
        view.select_branch("feature-b", window, cx);
    });
    cx.run_until_parked();
    cx.update(|_, app| {
        let state = view.read(app).workspace().unwrap().state();
        assert_eq!(state.selected_branch(), Some("feature-b"));
        assert_eq!(state.details(), &LoadState::Loading);
        assert_eq!(state.diff(), &LoadState::Loading);
        assert_eq!(state.ci(), &LoadState::Loading);
    });

    details_gate.release();
    diff_gate.release();
    cx.run_until_parked();

    cx.update(|_, app| {
        let state = view.read(app).workspace().unwrap().state();
        assert_eq!(state.selected_branch(), Some("feature-b"));
        assert_eq!(state.details().ready(), Some(&details(2, true)));
        assert_eq!(state.diff().ready(), Some(&diff("feature-b patch")));
        assert_eq!(state.ci().ready(), Some(&ci("success")));
    });
    assert!(!hydration.calls().iter().any(|call| matches!(
        call,
        HydrationCall::Ci { branch, .. } if branch == "feature-a"
    )));
}

#[gpui::test]
fn same_branch_retry_rejects_older_diff_and_ci_completions(cx: &mut TestAppContext) {
    let old_diff_gate = Arc::new(Gate::default());
    let old_ci_gate = Arc::new(Gate::default());
    let diff_calls = Arc::new(AtomicUsize::new(0));
    let ci_calls = Arc::new(AtomicUsize::new(0));
    let details_calls = Arc::new(AtomicUsize::new(0));
    let hydration = Arc::new(FixtureHydration::new_async(
        {
            let details_calls = Arc::clone(&details_calls);
            move |_, _| {
                let call = details_calls.fetch_add(1, Ordering::SeqCst);
                Box::pin(async move { Ok(details(call + 1, true)) })
            }
        },
        |_, _, _| Box::pin(async { Ok(None) }),
        {
            let old_diff_gate = Arc::clone(&old_diff_gate);
            let diff_calls = Arc::clone(&diff_calls);
            move |_, _, _| {
                let call = diff_calls.fetch_add(1, Ordering::SeqCst);
                let old_diff_gate = Arc::clone(&old_diff_gate);
                Box::pin(async move {
                    if call == 0 {
                        old_diff_gate.wait().await;
                        Ok(diff("stale retry patch"))
                    } else {
                        Ok(diff("current retry patch"))
                    }
                })
            }
        },
        {
            let old_ci_gate = Arc::clone(&old_ci_gate);
            let ci_calls = Arc::clone(&ci_calls);
            move |_, _| {
                let call = ci_calls.fetch_add(1, Ordering::SeqCst);
                let old_ci_gate = Arc::clone(&old_ci_gate);
                Box::pin(async move {
                    if call == 0 {
                        old_ci_gate.wait().await;
                        Ok(ci("failure"))
                    } else {
                        Ok(ci("success"))
                    }
                })
            }
        },
    ));
    let services = services_with_hydration(
        Ok(snapshot("/repo")),
        Ok(None),
        Arc::new(FixtureRecents::default()),
        hydration,
    );
    let (view, cx) = cx.add_window_view(|window, cx| {
        AppView::new(Some(PathBuf::from("/repo")), services, window, cx)
    });
    assert!(old_diff_gate.wait_until_started(Duration::from_secs(5)));
    assert!(old_ci_gate.wait_until_started(Duration::from_secs(5)));

    view.update_in(cx, |view, window, cx| {
        view.select_branch("feature-a", window, cx);
    });
    cx.run_until_parked();
    cx.update(|_, app| {
        let state = view.read(app).workspace().unwrap().state();
        assert_eq!(state.details().ready(), Some(&details(2, true)));
        assert_eq!(state.diff(), &LoadState::Loading);
        assert_eq!(state.ci(), &LoadState::Loading);
    });

    old_diff_gate.release();
    old_ci_gate.release();
    cx.run_until_parked();

    cx.update(|_, app| {
        let state = view.read(app).workspace().unwrap().state();
        assert_eq!(state.details().ready(), Some(&details(2, true)));
        assert_eq!(state.diff().ready(), Some(&diff("current retry patch")));
        assert_eq!(state.ci().ready(), Some(&ci("success")));
    });
}

#[gpui::test]
fn rapid_selections_bound_details_and_diff_to_active_plus_latest(cx: &mut TestAppContext) {
    let details_gate = Arc::new(Gate::default());
    let cache_gate = Arc::new(Gate::default());
    let details_active = Arc::new(AtomicUsize::new(0));
    let details_max = Arc::new(AtomicUsize::new(0));
    let cache_active = Arc::new(AtomicUsize::new(0));
    let cache_max = Arc::new(AtomicUsize::new(0));
    let hydration = Arc::new(FixtureHydration::new_async(
        {
            let gate = Arc::clone(&details_gate);
            let active = Arc::clone(&details_active);
            let max = Arc::clone(&details_max);
            move |_, branch| {
                let gate = Arc::clone(&gate);
                let active = Arc::clone(&active);
                let max = Arc::clone(&max);
                Box::pin(async move {
                    let current = active.fetch_add(1, Ordering::SeqCst) + 1;
                    max.fetch_max(current, Ordering::SeqCst);
                    if branch.name == "feature-a" {
                        gate.wait().await;
                    }
                    active.fetch_sub(1, Ordering::SeqCst);
                    Ok(details(
                        if branch.name == "feature-b" { 2 } else { 1 },
                        false,
                    ))
                })
            }
        },
        {
            let gate = Arc::clone(&cache_gate);
            let active = Arc::clone(&cache_active);
            let max = Arc::clone(&cache_max);
            move |_, branch, _| {
                let gate = Arc::clone(&gate);
                let active = Arc::clone(&active);
                let max = Arc::clone(&max);
                Box::pin(async move {
                    let current = active.fetch_add(1, Ordering::SeqCst) + 1;
                    max.fetch_max(current, Ordering::SeqCst);
                    if branch == "feature-a" {
                        gate.wait().await;
                    }
                    active.fetch_sub(1, Ordering::SeqCst);
                    Ok(None)
                })
            }
        },
        |_, branch, _| Box::pin(async move { Ok(diff(&format!("{branch} patch"))) }),
        |_, _| panic!("branches without remotes must not load CI"),
    ));
    let services = services_with_hydration(
        Ok(snapshot("/repo")),
        Ok(None),
        Arc::new(FixtureRecents::default()),
        hydration.clone(),
    );
    let (view, cx) = cx.add_window_view(|window, cx| {
        AppView::new(Some(PathBuf::from("/repo")), services, window, cx)
    });
    assert!(details_gate.wait_until_started(Duration::from_secs(5)));
    assert!(cache_gate.wait_until_started(Duration::from_secs(5)));

    view.update_in(cx, |view, window, cx| {
        for index in 0..50 {
            let branch = if index % 2 == 0 {
                "feature-b"
            } else {
                "feature-a"
            };
            view.select_branch(branch, window, cx);
        }
        view.select_branch("feature-b", window, cx);
    });
    cx.run_until_parked();

    let calls = hydration.calls();
    assert_eq!(
        calls
            .iter()
            .filter(|call| matches!(call, HydrationCall::Details { .. }))
            .count(),
        1
    );
    assert_eq!(
        calls
            .iter()
            .filter(|call| matches!(call, HydrationCall::CachedDiff { .. }))
            .count(),
        1
    );
    assert!(
        !calls
            .iter()
            .any(|call| matches!(call, HydrationCall::Diff { .. }))
    );

    details_gate.release();
    cache_gate.release();
    cx.run_until_parked();

    let calls = hydration.calls();
    let detail_branches = calls
        .iter()
        .filter_map(|call| match call {
            HydrationCall::Details { branch, .. } => Some(branch.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    let cache_branches = calls
        .iter()
        .filter_map(|call| match call {
            HydrationCall::CachedDiff { branch, .. } => Some(branch.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    let diff_branches = calls
        .iter()
        .filter_map(|call| match call {
            HydrationCall::Diff { branch, .. } => Some(branch.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(detail_branches, vec!["feature-a", "feature-b"]);
    assert_eq!(cache_branches, vec!["feature-a", "feature-b"]);
    assert_eq!(diff_branches, vec!["feature-b"]);
    assert_eq!(details_max.load(Ordering::SeqCst), 1);
    assert_eq!(cache_max.load(Ordering::SeqCst), 1);
    cx.update(|_, app| {
        let state = view.read(app).workspace().unwrap().state();
        assert_eq!(state.selected_branch(), Some("feature-b"));
        assert_eq!(state.details().ready(), Some(&details(2, false)));
        assert_eq!(state.diff().ready(), Some(&diff("feature-b patch")));
    });
}

#[gpui::test]
fn rapid_same_branch_retries_bound_ci_to_active_plus_latest(cx: &mut TestAppContext) {
    let ci_gate = Arc::new(Gate::default());
    let ci_active = Arc::new(AtomicUsize::new(0));
    let ci_max = Arc::new(AtomicUsize::new(0));
    let ci_calls = Arc::new(AtomicUsize::new(0));
    let hydration = Arc::new(FixtureHydration::new_async(
        |_, _| Box::pin(async { Ok(details(1, true)) }),
        |_, _, _| Box::pin(async { Ok(None) }),
        |_, branch, _| Box::pin(async move { Ok(diff(&format!("{branch} patch"))) }),
        {
            let gate = Arc::clone(&ci_gate);
            let active = Arc::clone(&ci_active);
            let max = Arc::clone(&ci_max);
            let calls = Arc::clone(&ci_calls);
            move |_, _| {
                let gate = Arc::clone(&gate);
                let active = Arc::clone(&active);
                let max = Arc::clone(&max);
                let calls = Arc::clone(&calls);
                Box::pin(async move {
                    let call = calls.fetch_add(1, Ordering::SeqCst);
                    let current = active.fetch_add(1, Ordering::SeqCst) + 1;
                    max.fetch_max(current, Ordering::SeqCst);
                    if call == 0 {
                        gate.wait().await;
                    }
                    active.fetch_sub(1, Ordering::SeqCst);
                    Ok(ci(if call == 0 { "failure" } else { "success" }))
                })
            }
        },
    ));
    let services = services_with_hydration(
        Ok(snapshot("/repo")),
        Ok(None),
        Arc::new(FixtureRecents::default()),
        hydration,
    );
    let (view, cx) = cx.add_window_view(|window, cx| {
        AppView::new(Some(PathBuf::from("/repo")), services, window, cx)
    });
    assert!(ci_gate.wait_until_started(Duration::from_secs(5)));

    view.update_in(cx, |view, window, cx| {
        for _ in 0..50 {
            view.select_branch("feature-a", window, cx);
        }
    });
    cx.run_until_parked();
    assert_eq!(ci_calls.load(Ordering::SeqCst), 1);

    ci_gate.release();
    cx.run_until_parked();

    assert_eq!(ci_calls.load(Ordering::SeqCst), 2);
    assert_eq!(ci_max.load(Ordering::SeqCst), 1);
    cx.update(|_, app| {
        let state = view.read(app).workspace().unwrap().state();
        assert_eq!(state.ci().ready(), Some(&ci("success")));
        let selected = state
            .snapshot()
            .branches
            .iter()
            .find(|branch| branch.name == "feature-a")
            .unwrap();
        assert_eq!(selected.ci_state.as_deref(), Some("success"));
    });
}

#[gpui::test]
fn replacing_repository_rejects_old_path_hydration_results(cx: &mut TestAppContext) {
    let old_details_gate = Arc::new(Gate::default());
    let old_diff_gate = Arc::new(Gate::default());
    let hydration = Arc::new(FixtureHydration::new_async(
        {
            let old_details_gate = Arc::clone(&old_details_gate);
            move |repository, _| {
                let old_details_gate = Arc::clone(&old_details_gate);
                Box::pin(async move {
                    if repository == Path::new("/repo") {
                        old_details_gate.wait().await;
                        Ok(details(99, true))
                    } else {
                        Ok(details(4, true))
                    }
                })
            }
        },
        |_, _, _| Box::pin(async { Ok(None) }),
        {
            let old_diff_gate = Arc::clone(&old_diff_gate);
            move |repository, _, _| {
                let old_diff_gate = Arc::clone(&old_diff_gate);
                Box::pin(async move {
                    if repository == Path::new("/repo") {
                        old_diff_gate.wait().await;
                        Ok(diff("old repository patch"))
                    } else {
                        Ok(diff("new repository patch"))
                    }
                })
            }
        },
        |_, _| Box::pin(async { Ok(ci("success")) }),
    ));
    let services = services_with_hydration(
        Ok(snapshot("/unused")),
        Ok(None),
        Arc::new(FixtureRecents::default()),
        hydration,
    );
    let (view, cx) = cx.add_window_view(|window, cx| AppView::new(None, services, window, cx));
    view.update_in(cx, |view, window, cx| {
        let token = view.begin_load(PathBuf::from("/repo"), RootLoadKind::Open);
        assert!(view.apply_load_result(token, Ok(snapshot("/repo")), cx));
        view.hydrate_selection(window, cx);
    });
    cx.run_until_parked();
    assert!(old_details_gate.wait_until_started(Duration::from_secs(5)));
    assert!(old_diff_gate.wait_until_started(Duration::from_secs(5)));

    view.update_in(cx, |view, window, cx| {
        let token = view.begin_load(PathBuf::from("/other"), RootLoadKind::Open);
        assert!(view.apply_load_result(token, Ok(snapshot("/other")), cx));
        view.hydrate_selection(window, cx);
    });
    cx.run_until_parked();
    old_details_gate.release();
    old_diff_gate.release();
    cx.run_until_parked();

    cx.update(|_, app| {
        let state = view.read(app).workspace().unwrap().state();
        assert_eq!(state.snapshot().repository_root, PathBuf::from("/other"));
        assert_eq!(state.details().ready(), Some(&details(4, true)));
        assert_eq!(state.diff().ready(), Some(&diff("new repository patch")));
        assert_eq!(state.ci().ready(), Some(&ci("success")));
    });
}

#[gpui::test]
fn refresh_retains_ready_diff_with_indicator_then_surfaces_failure(cx: &mut TestAppContext) {
    let refresh_diff_gate = Arc::new(Gate::default());
    let diff_calls = Arc::new(AtomicUsize::new(0));
    let hydration = Arc::new(FixtureHydration::new_async(
        |_, _| Box::pin(async { Ok(details(1, false)) }),
        |_, _, _| Box::pin(async { Ok(None) }),
        {
            let refresh_diff_gate = Arc::clone(&refresh_diff_gate);
            let diff_calls = Arc::clone(&diff_calls);
            move |_, _, _| {
                let call = diff_calls.fetch_add(1, Ordering::SeqCst);
                let refresh_diff_gate = Arc::clone(&refresh_diff_gate);
                Box::pin(async move {
                    if call == 0 {
                        Ok(diff("visible patch"))
                    } else {
                        refresh_diff_gate.wait().await;
                        Err("fresh diff failed".into())
                    }
                })
            }
        },
        |_, _| Box::pin(async { Ok(ci("success")) }),
    ));
    let services = services_with_hydration(
        Ok(snapshot("/repo")),
        Ok(None),
        Arc::new(FixtureRecents::default()),
        hydration,
    );
    let (view, cx) = cx.add_window_view(|window, cx| {
        AppView::new(Some(PathBuf::from("/repo")), services, window, cx)
    });

    view.update_in(cx, |view, window, cx| {
        view.refresh_repository(window, cx);
    });
    cx.run_until_parked();
    assert!(refresh_diff_gate.wait_until_started(Duration::from_secs(5)));
    cx.update(|_, app| {
        let state = view.read(app).workspace().unwrap().state();
        assert_eq!(state.selected_branch(), Some("feature-a"));
        assert_eq!(state.diff().ready(), Some(&diff("visible patch")));
        assert!(state.diff_is_refreshing());
    });
    assert!(cx.debug_bounds("changes-refreshing").is_some());

    refresh_diff_gate.release();
    cx.run_until_parked();

    cx.update(|_, app| {
        let state = view.read(app).workspace().unwrap().state();
        assert_eq!(state.diff().error(), Some("fresh diff failed"));
        assert!(!state.diff_is_refreshing());
    });
}

#[gpui::test]
fn cached_diff_is_shown_for_new_selection_until_fresh_diff_replaces_it(cx: &mut TestAppContext) {
    let feature_b_diff_gate = Arc::new(Gate::default());
    let hydration = Arc::new(FixtureHydration::new_async(
        |_, branch| Box::pin(async move { Ok(details(1, branch.name == "feature-b")) }),
        |_, branch, _| {
            Box::pin(
                async move { Ok((branch == "feature-b").then(|| diff("cached feature-b patch"))) },
            )
        },
        {
            let feature_b_diff_gate = Arc::clone(&feature_b_diff_gate);
            move |_, branch, _| {
                let feature_b_diff_gate = Arc::clone(&feature_b_diff_gate);
                Box::pin(async move {
                    if branch == "feature-b" {
                        feature_b_diff_gate.wait().await;
                        Ok(diff("fresh feature-b patch"))
                    } else {
                        Ok(diff("feature-a patch"))
                    }
                })
            }
        },
        |_, _| Box::pin(async { Ok(ci("success")) }),
    ));
    let services = services_with_hydration(
        Ok(snapshot("/repo")),
        Ok(None),
        Arc::new(FixtureRecents::default()),
        hydration,
    );
    let (view, cx) = cx.add_window_view(|window, cx| {
        AppView::new(Some(PathBuf::from("/repo")), services, window, cx)
    });

    view.update_in(cx, |view, window, cx| {
        view.select_branch("feature-b", window, cx);
    });
    cx.run_until_parked();
    assert!(feature_b_diff_gate.wait_until_started(Duration::from_secs(5)));
    cx.update(|_, app| {
        let state = view.read(app).workspace().unwrap().state();
        assert_eq!(state.diff().ready(), Some(&diff("cached feature-b patch")));
        assert!(state.diff_is_refreshing());
    });

    feature_b_diff_gate.release();
    cx.run_until_parked();

    cx.update(|_, app| {
        let state = view.read(app).workspace().unwrap().state();
        assert_eq!(state.diff().ready(), Some(&diff("fresh feature-b patch")));
        assert!(!state.diff_is_refreshing());
    });
}

#[gpui::test]
fn hydration_completion_after_window_close_is_harmless(cx: &mut TestAppContext) {
    let details_gate = Arc::new(Gate::default());
    let diff_gate = Arc::new(Gate::default());
    let hydration = Arc::new(FixtureHydration::new_async(
        {
            let details_gate = Arc::clone(&details_gate);
            move |_, _| {
                let details_gate = Arc::clone(&details_gate);
                Box::pin(async move {
                    details_gate.wait().await;
                    Ok(details(1, true))
                })
            }
        },
        |_, _, _| Box::pin(async { Ok(None) }),
        {
            let diff_gate = Arc::clone(&diff_gate);
            move |_, _, _| {
                let diff_gate = Arc::clone(&diff_gate);
                Box::pin(async move {
                    diff_gate.wait().await;
                    Ok(diff("late patch"))
                })
            }
        },
        |_, _| Box::pin(async { Ok(ci("success")) }),
    ));
    let services = services_with_hydration(
        Ok(snapshot("/repo")),
        Ok(None),
        Arc::new(FixtureRecents::default()),
        hydration,
    );
    let (view, cx) = cx.add_window_view(|window, cx| {
        AppView::new(Some(PathBuf::from("/repo")), services, window, cx)
    });
    assert!(details_gate.wait_until_started(Duration::from_secs(5)));
    assert!(diff_gate.wait_until_started(Duration::from_secs(5)));

    cx.update(|window, _| window.remove_window());
    drop(view);
    details_gate.release();
    diff_gate.release();
    cx.run_until_parked();
}
