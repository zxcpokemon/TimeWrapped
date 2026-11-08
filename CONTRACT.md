# TimeWrapped 1.0.1 integration

IPC protocol 12, SQLite schema 4. Old collectors commit/exit before migration. Settings/shutdown failures propagate. Native collector and tray never create WebView; GUI is separate.

Streams:

- computer: all observed uptime; sleep and locked are subsets.
- active: recognized pre-idle focused app; idle: unlocked inactivity; unattributed: unknown or uncertain focus.
- visible: parallel exposure while an ordinary window has visible area on any monitor, regardless of keyboard idle.
- running: awake process lifetime, union of discovered user-session applications and visible identities, deduplicated by executable.
- background: running while not visible, including minimized/covered/no-window/locked state. It is a subset of running, never added on top of running.
- music: confirmed Spotify Playing plus a matching active local WASAPI session and unmuted/nonzero app and endpoint volume. Independent of visibility and focus, stopped during suspend or tracking pause. No titles/artwork/content metadata.
- legacy/legacy_idle: old data preserved without pretending to know historical visibility or hidden processes.

PC total = computer+legacy+legacy_idle. Visible app total = visible+legacy. AppUsage.seconds is visible total; focused_seconds, running_seconds, background_seconds and music_seconds are separate. Dashboard.spotify_seconds sums included Spotify music only. Categories and favorites use visible total, chronotype uses active focus. Exclusions affect app lanes, never the PC clock.

Migration 2→3 backs up the old database, preserves all prior segment rows/IDs/settings, widens the stream constraint and backfills running from known visible/legacy intervals only. No inferred old background or music. Export schema is 4. Clear-data removes owned migration backups after stopping/flush and clearing history.

Scene changes coalesce for 250 ms. DWM bounds fall back to GetWindowRect when empty; tool/desktop/taskbar windows are discarded before occlusion. Physical monitor rectangles support mixed DPI/negative coordinates. No captions, screenshots, pixels or input content are collected.

User-session process discovery runs every 15 s, cached handles checked every 5 s, bounded to 768 entries. System/known helper processes are filtered. Short windowless processes can be missed between scans. SMTC session events wake one media thread, with local audio recheck every 5 s. SQLite batching remains about 45 s; GUI refresh also flushes.

Diagnostics include protocol_version, boot_seconds, awake_since_boot_seconds, monitor_count, visible_app_count, running_app_count, process_inventory_scans, running_sample_interval_seconds, spotify_status, spotify_media_available and spotify_playing. CPU is null during the first second. GUI status is a snapshot refreshed with statistics.


## Optional extensions (0.4)

The screen-time database uses schema 4. `extensions.sqlite3` stores separate opt-in config, track intervals, game caches and a DPAPI-protected FACEIT key. Defaults collect no track titles and perform no game requests. Main settings add `autostart_open_window` with serde default false; the Run entry can start either the background collector or GUI.

Spotify titles/artist are read through SMTC only after enabling the history extension, and only active, locally unmuted playback is timed. Track changes and playback changes wake the media worker. Intervals use unbiased Windows awake time; suspend and uncertain gaps are skipped. Global tracking pause also stops track history.

SoundCloud uses the bundled MV3 browser add-on. It reads only the SoundCloud player's title, artist and playing state; a service worker posts authenticated heartbeats to 127.0.0.1:38671. The bridge is loopback-only, bounded in size/time, rejects arbitrary website origins and requires a random per-installation pairing token. Browser playback state does not prove audible system output.

A user supplies one public Steam profile URL or SteamID64 for games. Numeric IDs are validated and converted to Dota account IDs. Vanity URLs use Steam's deprecated public XML route and may fail; numeric /profiles/ URLs remain available. No Steam OAuth, passwords, cookies or Steam API key. HTTPS requests use WinHTTP with fixed service hosts, normal certificate validation, timeouts, response size limits and disabled redirects.

Dota uses public OpenDota profile, recentMatches, wl, heroes, peers and hero constants. Dotabuff/OpenDota/STRATZ links open in the user's browser. FACEIT uses the official authenticated Data API to resolve a CS2 profile by SteamID and fetch stats/history. A user-provided API key is required only for Data API mode. Version 0.5 adds a public mode without a key. No server or shared built-in key.

Game refresh runs about every 15 minutes, including while GUI is closed; manual refresh is throttled to one minute. Errors retain the last successful cache with an explicit stale/error state. FACEIT keys are never included in the screen-time export or frontend state. The loopback pairing token is intentionally shown in the SoundCloud setup UI. Clear-data also clears tracks and game cache, retaining extension configuration and keys.


## Browsing, private reports and genres (0.5)

Config gains sites_history, site_details, adult_stats, adult_domains, excluded_sites, music_genres, genres_online and faceit_mode. All new collection and online genre lookup flags default false. FACEIT defaults to public mode when faceit_mode is empty/public, with api as an explicit alternative. Public mode calls FACEIT web-client JSON routes without a key and normalizes only needed profile/game fields; it does not store the returned friends/social graph.

site_sessions and artist_genres are additive tables in extensions.sqlite3. Browsing intervals require both app opt-in and browser optional permissions. Only the focused HTTP(S) tab is reported; incognito is prohibited by the manifest and rejected by the background script. URL credentials, query and fragment are discarded. Page path/title are stored only with site_details. Known 18+ hosts are dropped unless adult_stats is enabled; exclusions take precedence, including over custom 18+ domains. Domain comparisons use exact/suffix boundaries.

The authenticated loopback bridge exposes GET /browser-config and POST /visit in addition to SoundCloud. Site time uses bounded awake-clock intervals, global pause and WTS lock state; sleep, lock and uncertain gaps add no browsing time. Native durations are authoritative, never client-submitted timestamps. A random per-installation token is still required; only extension origins receive CORS headers. Isolated tests bind an ephemeral loopback port.

get_extension_report returns clipped site/music durations, daily data, per-site totals and selected-site history for a local date range of at most 366 days. Histories show up to 300 sessions for the selected site. Weekly Wrapped includes extension report data and extra genre/adult slides only when enabled; the general PNG card omits private web statistics. A separate delete command removes adult rows only; clear-all removes all browsing rows and genre maps with the other extension history.

Genre lookup is opt-in and sends artist names only to MusicBrainz; global SQLite leases enforce at most one request per 1.1 seconds. Exact ambiguous names or absent genres remain unknown. Manual artist genres take priority. Reports show coverage and explicitly treat these as artist genres, not guaranteed track-level genres. Manual and weekly/monthly reports work without online lookup.

## Application catalog (1.0)

`app_catalog` is the positive application registry. Inventory admits processes only after an ordinary top-level window has been observed, or when an already known app root process is running. Minimized/covered main windows qualify; tool/no-activate/owned-only windows do not. Same-executable children without a main window do not establish a running app lifetime. Shared policy excludes known supporting processes on discovery, foreground collection and writes. Steam helper windows map to steam.exe.

Migration 3→4 creates activity.pre-v1.0.sqlite3 with VACUUM INTO, retains every original segment/bucket/app/settings row, seeds the catalog from actual historical visible/focus/music lanes and removes known helpers from eligibility. Running-only old processes are not evidence of an app. Lists, category summaries and application export require catalog membership; computer totals stay independent. Proven minimized apps can be registered before their first exposure interval.

## Unified browser companion (1.0)

One Manifest V3 package has a stable public key, popup and full-page settings using the same controller. Local token storage is shared across all features. Authenticated GET /browser-config reports version/global pause and the five collection flags. Authenticated POST /browser-settings accepts only boolean fields sites_history, soundcloud, spotify_titles, site_details, adult_stats; every other field/type is rejected. It cannot change profiles, API keys, autostart or the global tracking switch. Content scripts cannot invoke companion control messages: sender URL must exactly match this extension's popup/options page.

Site access is requested only from an explicit checkbox gesture. No browser history permission or retrospective import. Local browser pause suppresses website and SoundCloud transport; Spotify remains controlled by the native app. A disabled/revoked optional permission prevents site collection. Existing tabs receive the site script after an explicit enable action; SoundCloud already open during installation needs reload.

Clock mapping in 1.0 anchors each normal sample to the previous monotonic timeline endpoint; wall-clock jitter does not introduce gaps/overlaps or fragment coalescing. Real wall-clock changes beyond two seconds re-anchor labels and leave the uncertain interval unattributed. Zero-uptime ticks do not move the timeline. Pause re-anchors without inventing paused history.

## Browser regression fix (1.0.1)

Application sites_history plus actual optional tabs/host grants are authoritative; the obsolete historyEnabled local key no longer gates capture. Serialized synchronization restores registrations and active existing tabs on pair/start/activation/permission changes, and a 30-second alarm retries connection after native collector restarts. SoundCloud is reinjected/sampled in existing tabs on the same alarm with a per-tab reporter singleton. Required scripting+alarms and existing SoundCloud host scope support this; all-site access remains optional.

SoundCloud uses the canonical play control and timeline, not the first arbitrary audio element. Heartbeats include optional position seconds. Native Journal keeps the normal 8-second limit; gaps up to 75 seconds require the same playing track and a valid non-seek playhead delta, and count at most min(awake elapsed, playhead advance). Clock/sleep mismatch, no progress, unknown position or long pause are not inferred. Spotify remains on its original short-gap rule.

## Popup liveness (1.0.2)

Native HTTP protocol and SQLite schema are unchanged (IPC 12, schema 4). Companion config/settings replies authenticate with the native bridge and read bounded local permission/state APIs. They do not await script injection or a preexisting maintenance promise. Browser script work is deferred, parallel across services/tabs, bounded per API and per maintenance job; latest settings are queued after pending maintenance. injectImmediately avoids waiting for document_idle in existing tabs. Popup messaging has a 6.5-second deadline, storage has a 2-second deadline, and retry controls are released in finally. HTTP, JSON-body and Chrome API waits also have deadlines.

## Browser history and locales (1.1.0)

Primary schema 5 blocks older writers from reopening the upgraded data. Collector IPC 13 ensures old collection is flushed before migration. Existing primary and browsing databases get owned pre-v1.1.sqlite3 backups.

site_sessions preserves the original timed intervals. site_visits stores stable navigation identities, visited/last_seen, domain/title/path and a reopenable sanitized URL. New clients retain an ID across blur/focus, create one on document/SPA navigation, retry pending navigation and update titles without duplicating visits. Native report totals still sum timed intervals; recent now contains visits without per-entry seconds. Unicode search and site/adult filters affect the history list; total history count and up to 300 rows are returned.

Migration seeds visits once, grouping legacy fragments of the same domain/title (or path without title) within 30 minutes. It never edits original time rows. Legacy clients without a navigation flag receive the same approximate page deduplication. New explicit navigations remain distinct. Private/all-history deletion clears both tables and the owned migration backup.

Website details keep only a valid YouTube video ID from query parameters; all other query/fragment data stays excluded. Opening history uses only an existing visit ID, validates its stored public HTTP(S) URL and invokes the default browser.

Settings.language is validated ru/en/uk and serde-defaults to ru for old data. Browser config exposes it; the browser panel may save a local override. Source-owned strings and template skeletons are translated before interpolating user values. App names, page titles and artist/track names are not dictionary inputs. Shared catalogs cover frontend/browser/native tray, date and number formatting uses the selected locale, and build checks enforce identical catalogs.
