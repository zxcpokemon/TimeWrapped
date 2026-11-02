# TimeWrapped 0.3 integration

IPC protocol 5, SQLite schema 3. Old collectors commit/exit before migration. Settings/shutdown failures propagate. Native collector and tray never create WebView; GUI is separate.

Streams:

- computer: all observed uptime; sleep and locked are subsets.
- active: recognized pre-idle focused app; idle: unlocked inactivity; unattributed: unknown or uncertain focus.
- visible: parallel exposure while an ordinary window has visible area on any monitor, regardless of keyboard idle.
- running: awake process lifetime, union of discovered user-session applications and visible identities, deduplicated by executable.
- background: running while not visible, including minimized/covered/no-window/locked state. It is a subset of running, never added on top of running.
- music: confirmed Spotify Playing plus a matching active local WASAPI session and unmuted/nonzero app and endpoint volume. Independent of visibility and focus, stopped during suspend or tracking pause. No titles/artwork/content metadata.
- legacy/legacy_idle: old data preserved without pretending to know historical visibility or hidden processes.

PC total = computer+legacy+legacy_idle. Visible app total = visible+legacy. AppUsage.seconds is visible total; focused_seconds, running_seconds, background_seconds and music_seconds are separate. Dashboard.spotify_seconds sums included Spotify music only. Categories and favorites use visible total, chronotype uses active focus. Exclusions affect app lanes, never the PC clock.

Migration 2→3 backs up the old database, preserves all prior segment rows/IDs/settings, widens the stream constraint and backfills running from known visible/legacy intervals only. No inferred old background or music. Export schema is 3. Clear-data removes owned migration backups after stopping/flush and clearing history.

Scene changes coalesce for 250 ms. DWM bounds fall back to GetWindowRect when empty; tool/desktop/taskbar windows are discarded before occlusion. Physical monitor rectangles support mixed DPI/negative coordinates. No captions, screenshots, pixels or input content are collected.

User-session process discovery runs every 15 s, cached handles checked every 5 s, bounded to 768 entries. System/known helper processes are filtered. Short windowless processes can be missed between scans. SMTC session events wake one media thread, with local audio recheck every 5 s. SQLite batching remains about 45 s; GUI refresh also flushes.

Diagnostics include protocol_version, boot_seconds, awake_since_boot_seconds, monitor_count, visible_app_count, running_app_count, process_inventory_scans, running_sample_interval_seconds, spotify_status, spotify_media_available and spotify_playing. CPU is null during the first second. GUI status is a snapshot refreshed with statistics.
