mod fixture;
use fixture::{FeedErrorHost, FeedHost, GuessHost, LastHost, SeasonHost, start};
use nsh_engine::Root;

#[test]
fn a_slow_relay_coalesces_duplicate_ends_and_uses_the_first_actual_episode() -> Result<(), String> {
    let mut app = start(Root::of::<SeasonHost>());
    app.wait_for("Advance")?;
    app.press("Advance")?;
    app.wait_for("text:requests:1 pending:true targets: feed:0 paused:false discarded:0")?;
    app.press("Advance")?;
    app.wait_for("text:requests:1 pending:true targets: feed:0 paused:false discarded:0")?;
    app.press("Complete")?;
    app.wait_for(
        "text:requests:1 pending:false targets:resolved:3 feed:0 paused:false discarded:0",
    )?;
    app.press("Advance")?;
    app.wait_for(
        "text:requests:1 pending:false targets:resolved:3 feed:0 paused:false discarded:0",
    )?;
    Ok(())
}

#[test]
fn a_new_source_cancels_the_old_result_without_clearing_the_new_request() -> Result<(), String> {
    let mut app = start(Root::of::<SeasonHost>());
    app.wait_for("Advance")?;
    app.press("Advance")?;
    app.wait_for("text:requests:1 pending:true targets: feed:0 paused:false discarded:0")?;
    app.press("Source")?;
    app.wait_for("text:requests:1 pending:false targets: feed:0 paused:false discarded:0")?;
    app.press("Advance")?;
    app.wait_for("text:requests:2 pending:true targets: feed:0 paused:false discarded:0")?;
    app.press("Complete")?;
    app.wait_for("text:requests:2 pending:true targets: feed:0 paused:false discarded:1")?;
    app.press("Complete")?;
    app.wait_for(
        "text:requests:2 pending:false targets:resolved:3 feed:0 paused:false discarded:1",
    )?;
    Ok(())
}

#[test]
fn account_download_pause_route_and_feed_changes_cancel_inflight_resolution() -> Result<(), String>
{
    for change in ["Account", "Download", "Pause", "Leave", "Feed context"] {
        let mut app = start(Root::of::<SeasonHost>());
        app.wait_for("Advance")?;
        app.press("Advance")?;
        app.wait_for("text:requests:1 pending:true targets: feed:0 paused:false discarded:0")?;
        app.press(change)?;
        let paused = change == "Pause";
        app.wait_for(&format!(
            "text:requests:1 pending:false targets: feed:0 paused:{paused} discarded:0"
        ))?;
        app.press("Complete")?;
        app.wait_for(&format!(
            "text:requests:1 pending:false targets: feed:0 paused:{paused} discarded:1"
        ))?;
    }
    Ok(())
}

#[test]
fn download_blocks_new_relay_and_resolve_errors_release_busy_without_switching()
-> Result<(), String> {
    let mut app = start(Root::of::<SeasonHost>());
    app.wait_for("Advance")?;
    app.press("Download")?;
    app.press("Advance")?;
    app.wait_for("text:requests:0 pending:false targets: feed:0 paused:false discarded:0")?;
    let mut app = start(Root::of::<SeasonHost>());
    app.wait_for("Advance")?;
    app.press("Advance")?;
    app.wait_for("text:requests:1 pending:true targets: feed:0 paused:false discarded:0")?;
    app.press("Fail")?;
    app.wait_for("text:requests:1 pending:false targets: feed:0 paused:true discarded:0")?;
    Ok(())
}

#[test]
fn feed_advance_skips_guess_resolution_and_standalone_resolves_guess_or_stops() -> Result<(), String>
{
    let mut app = start(Root::of::<FeedHost>());
    app.wait_for("Advance")?;
    app.press("Advance")?;
    app.wait_for("text:requests:0 pending:false targets: feed:1 paused:false discarded:0")?;
    app.press("Advance")?;
    app.wait_for("text:requests:0 pending:false targets: feed:1 paused:false discarded:0")?;
    let mut app = start(Root::of::<GuessHost>());
    app.wait_for("Advance")?;
    app.press("Advance")?;
    app.wait_for("text:requests:1 pending:true targets: feed:0 paused:false discarded:0")?;
    app.press("Complete")?;
    app.wait_for(
        "text:requests:1 pending:false targets:resolved:3 feed:0 paused:false discarded:0",
    )?;
    let mut app = start(Root::of::<LastHost>());
    app.wait_for("Advance")?;
    app.press("Advance")?;
    app.wait_for("text:requests:0 pending:false targets: feed:0 paused:true discarded:0")?;
    Ok(())
}

#[test]
fn a_failed_related_query_keeps_the_tauri_feed_fallback_available() -> Result<(), String> {
    let mut app = start(Root::of::<FeedErrorHost>());
    app.wait_for("Advance")?;
    app.press("Advance")?;
    app.wait_for("text:requests:0 pending:false targets: feed:1 paused:false discarded:0")?;
    Ok(())
}
