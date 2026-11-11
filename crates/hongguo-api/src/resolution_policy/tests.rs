use super::*;
use serde_json::json;

fn candidates(values: &[u32]) -> Vec<Candidate> {
    values
        .iter()
        .map(|&definition| Candidate {
            definition,
            bitrate: definition * 1000,
            file_hash: None,
        })
        .collect()
}

fn select(
    values: &[u32],
    preferred: Option<u32>,
    cache: Option<u32>,
    purpose: Purpose,
) -> Option<u32> {
    Rules::default().select(
        &Selection {
            candidates: &candidates(values),
            preferred,
            cached: cache,
            purpose,
            strategy: &Strategy::default(),
            environment: Environment::default(),
            now_ms: 0,
        },
        &mut HeldStrategy::default(),
    )
}

#[test]
fn automatic_uses_the_official_priority_and_handles_missing_definitions() {
    assert_eq!(
        select(&DEFINITION_ORDER, None, None, Purpose::Playback),
        Some(720)
    );
    assert_eq!(
        select(&DEFINITION_ORDER, Some(540), Some(1080), Purpose::Playback),
        Some(540)
    );
    assert_eq!(
        select(&DEFINITION_ORDER, None, Some(1080), Purpose::Playback),
        Some(1080)
    );
    assert_eq!(
        select(&[1080, 540], None, None, Purpose::Playback),
        Some(1080)
    );
    assert_eq!(
        select(&[1080, 540], None, None, Purpose::Prefetch),
        Some(540)
    );
    assert_eq!(
        select(&[360, 480], None, None, Purpose::Playback),
        Some(360)
    );
    assert_eq!(select(&[1080], None, None, Purpose::Playback), Some(1080));
    assert_eq!(select(&[], None, None, Purpose::Playback), None);
}

fn run(
    rules: &Rules,
    strategy: &Strategy,
    env: Environment,
    purpose: Purpose,
    now: u64,
    held: &mut HeldStrategy,
) -> Option<u32> {
    rules.select(
        &Selection {
            candidates: &candidates(&DEFINITION_ORDER),
            preferred: None,
            cached: None,
            purpose,
            strategy,
            environment: env,
            now_ms: now,
        },
        held,
    )
}

fn strategy(rule: Value) -> Strategy {
    Strategy {
        rules: Some(vec![serde_json::from_value(rule).unwrap()]),
        ..Default::default()
    }
}

#[test]
fn server_upgrade_peak_demotion_and_keep_alive_obey_official_precedence() {
    let mut rules = Rules::default();
    rules.upgrade.enable_server = true;
    rules.server.enable_server = true;
    let mut strategy = strategy(json!({"suggest_resolution_type":"1080P","keep_alive_duration":2}));
    let mut held = HeldStrategy::default();
    let env = Environment {
        episode: 13,
        ..Default::default()
    };
    assert_eq!(
        run(&rules, &strategy, env, Purpose::Playback, 1000, &mut held),
        Some(1080)
    );
    strategy.rules = Some(vec![]);
    assert_eq!(
        run(&rules, &strategy, env, Purpose::Playback, 2999, &mut held),
        Some(1080)
    );
    assert_eq!(
        run(&rules, &strategy, env, Purpose::Playback, 3000, &mut held),
        Some(720)
    );
    strategy.rules = Some(vec![
        serde_json::from_value(json!({"suggest_resolution_type":"1080P"})).unwrap(),
    ]);
    strategy.is_peak_period = true;
    strategy.peak_demotion = Some(PeakDemotion {
        resolution: Some("480P".into()),
        resolution_in_peak_min_episode_index: 12,
    });
    assert_eq!(
        run(&rules, &strategy, env, Purpose::Playback, 4000, &mut held),
        Some(480)
    );
    strategy.rules.as_mut().unwrap()[0].ignore_peak = true;
    assert_eq!(
        run(&rules, &strategy, env, Purpose::Playback, 5000, &mut held),
        Some(1080)
    );
    strategy.rules = None;
    assert_eq!(
        run(
            &rules,
            &strategy,
            Environment { episode: 12, ..env },
            Purpose::Playback,
            6000,
            &mut held
        ),
        Some(720)
    );
}

#[test]
fn network_branch_ignores_upgrade_and_applies_peak_and_preload_floor() {
    let mut rules = Rules::default();
    rules.upgrade.enable_server = true;
    rules.network.enable_net_resolution = true;
    rules.network.net_speed_type = 2;
    rules.network.lower_time_preload_best_quality = true;
    let strategy = strategy(json!({"suggest_resolution_type":"1080P"}));
    let mut held = HeldStrategy::default();
    // type 2 先按整 Mbps 截断；未知/不足 1 Mbps 不应盲目升级。
    let mut env = Environment {
        predictor_bps: 900_000.0,
        ..Default::default()
    };
    assert_eq!(
        run(&rules, &strategy, env, Purpose::Playback, 1000, &mut held),
        Some(720)
    );
    env.predictor_bps = 1_500_000.0;
    assert_eq!(
        run(&rules, &strategy, env, Purpose::Playback, 2000, &mut held),
        Some(720)
    );
    rules.network.net_speed_type = 1;
    env.target_mbps = 0.55;
    assert_eq!(
        run(&rules, &strategy, env, Purpose::Playback, 3000, &mut held),
        Some(540)
    );
    assert_eq!(
        run(&rules, &strategy, env, Purpose::Prefetch, 3000, &mut held),
        Some(720)
    );
    env.peak = true;
    rules.peak.resolution_type = 2;
    assert_eq!(
        run(&rules, &strategy, env, Purpose::Prefetch, 3000, &mut held),
        Some(480)
    );
    env.target_mbps = 0.1;
    rules.network.not_match_use_smallest = true;
    assert_eq!(
        run(&rules, &strategy, env, Purpose::Playback, 3000, &mut held),
        Some(360)
    );
}

#[test]
fn conditional_rules_use_wifi_speed_and_digg_interceptor() {
    let mut rules = Rules::default();
    rules.upgrade.enable_server = true;
    let strategy = strategy(
        json!({"suggest_resolution_type":"1080P", "limit_condition":{
        "is_wifi":true,"check_net_speed":true,"net_speed":{"check_net_type":1,"network_speed":150}}}),
    );
    let mut held = HeldStrategy::default();
    let mut env = Environment {
        wifi: true,
        predictor_bps: 2_000_000.0,
        ..Default::default()
    };
    assert_eq!(
        run(&rules, &strategy, env, Purpose::Playback, 1000, &mut held),
        Some(1080)
    );
    env.wifi = false;
    assert_eq!(
        run(&rules, &strategy, env, Purpose::Playback, 1000, &mut held),
        Some(720)
    );
    env.wifi = true;
    env.predictor_bps = 999_999.0;
    assert_eq!(
        run(&rules, &strategy, env, Purpose::Playback, 1000, &mut held),
        Some(720)
    );
    env.predictor_bps = 2_000_000.0;
    rules.upgrade.digg_strategy = true;
    assert_eq!(
        run(&rules, &strategy, env, Purpose::Playback, 1000, &mut held),
        Some(720)
    );
}

#[test]
fn config_sources_and_shortplay_polynomial_are_not_fixed_to_720p() {
    let mut settings = Sources::read(
        &json!({"resolution_net_strategy_v649":{"enable_net_resolution":true},
        "engine_net_report_v649":{"bitrate_opt_function":{"ShortPlay":"0,0,0,1.1"}}}),
        false,
    )
    .unwrap();
    settings.merge(
        Sources::read(
            &json!({"series_resolution_v635":{"single_series":true}}),
            false,
        )
        .unwrap(),
    );
    let ab = Sources::read(
        &json!({"series_resolution_v635":{"val":"{\"supported_definitions\":\"1080P,720P\"}"}}),
        true,
    )
    .unwrap();
    let rules = Rules::from_sources(&ab, &settings);
    assert_eq!(rules.allowed(), vec![720, 1080]);
    assert!(
        !rules.series.single_series,
        "AB 替代整份配置，不与 settings 字段拼接"
    );
    let env = Environment {
        target_mbps: 0.5,
        ..Default::default()
    };
    assert_eq!(
        run(
            &rules,
            &Strategy::default(),
            env,
            Purpose::Playback,
            1000,
            &mut HeldStrategy::default()
        ),
        Some(1080)
    );
}

#[test]
fn filtered_allowed_definitions_control_network_peak_and_prefetch_comparisons() {
    let mut rules = Rules::default();
    rules.network.enable_net_resolution = true;
    rules.server.enable_server = true;
    rules.series.supported_definitions = "720P,1080P".into();
    let strategy = Strategy {
        is_peak_period: true,
        peak_demotion: Some(PeakDemotion {
            resolution: Some("480P".into()),
            ..Default::default()
        }),
        ..Default::default()
    };
    let env = Environment {
        peak: true,
        target_mbps: 2.0,
        ..Default::default()
    };
    assert_eq!(
        run(
            &rules,
            &strategy,
            env,
            Purpose::Playback,
            1000,
            &mut HeldStrategy::default()
        ),
        Some(1080)
    );
    rules.series.supported_definitions = "540P,1080P".into();
    rules.network.lower_time_preload_best_quality = true;
    assert_eq!(
        run(
            &rules,
            &strategy,
            Environment {
                peak: false,
                target_mbps: 0.55,
                ..env
            },
            Purpose::Prefetch,
            1000,
            &mut HeldStrategy::default()
        ),
        Some(540)
    );
}

#[test]
fn present_but_empty_server_peak_configuration_does_not_enable_local_demotion() {
    let mut rules = Rules::default();
    rules.server.enable_server = true;
    rules.peak.resolution_type = 2;
    let mut strategy = Strategy {
        peak_demotion: Some(PeakDemotion::default()),
        ..Default::default()
    };
    let env = Environment {
        peak: true,
        ..Default::default()
    };
    assert_eq!(
        run(
            &rules,
            &strategy,
            env,
            Purpose::Playback,
            1000,
            &mut HeldStrategy::default()
        ),
        Some(720)
    );
    strategy.is_peak_period = true;
    strategy.peak_demotion.as_mut().unwrap().resolution = Some("540p".into());
    assert_eq!(
        run(
            &rules,
            &strategy,
            env,
            Purpose::Playback,
            1000,
            &mut HeldStrategy::default()
        ),
        Some(720),
        "服务端高峰档位按 h(String) 严格匹配大写 P"
    );
}
