//! The status page: the problems first, the last runs, how each publisher
//! hears each tag over the last hour and day, and the publishers. Static
//! HTML without scripts, which reloads itself every minute.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::time::Duration;

use serde_json::{Value, json};

use super::Recorder;
use crate::history::DAY;
use crate::site::{Cell, Grade, Problem, Site, label, plural};
use crate::time;

const HOUR: u64 = 3600;
/// A publisher without diagnostics for longer has a problem. Instances send
/// them every minute, boards every cycle.
const SILENT: u64 = 300;

const STYLE: &str = "\
:root{color-scheme:light dark;--fg:#1d1d1f;--bg:#fff;--muted:#6e6e73;--line:#d2d2d7;\
--fair:#9a5b00}
@media (prefers-color-scheme:dark){:root{--fg:#f2f2f4;--bg:#17171a;--muted:#a1a1a6;\
--line:#3a3a3e;--fair:#ffb340}}
body{font:15px/1.45 system-ui,sans-serif;color:var(--fg);background:var(--bg);\
max-width:1100px;margin:0 auto;padding:16px}
h1{font-size:1.4em;margin:0 0 4px}
h2{font-size:1.1em;margin:28px 0 8px}
.meta,small,h2 small{color:var(--muted);font-weight:400}
table{border-collapse:collapse;display:block;overflow-x:auto}
th,td{padding:4px 14px 4px 0;text-align:left;vertical-align:top;\
border-bottom:1px solid var(--line);white-space:nowrap}
th{font-weight:600}
td.wrap{white-space:normal;min-width:16em}
.banner{border-left:3px solid var(--fair);padding:2px 10px}
ul{list-style:none;padding:0;margin:0}
";

/// The page, and its problems as JSON for the runner's notifications
pub struct Page {
    pub html: String,
    pub problems: Value,
}

pub fn render(
    r: &mut Recorder,
    now: u64,
    title: &str,
    expect: &[(String, String)],
    runs: &Result<Option<Value>, String>,
) -> Page {
    let names: BTreeMap<String, String> = r
        .devices
        .iter()
        .filter_map(|(id, d)| Some((id.clone(), d.name.clone()?)))
        .collect();
    let hour = Site::from_parts(names.clone(), r.history.window(now, HOUR));
    let day = Site::from_parts(names, r.history.window(now, DAY));
    let problems = problems(r, now, expect, &hour);
    track(r, now, &problems);
    let r = &*r;

    let mut out = String::new();
    let title = esc(title);
    let _ = write!(
        out,
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
         <meta http-equiv=\"refresh\" content=\"60\">\n<title>{title}</title>\n\
         <style>\n{STYLE}</style>\n</head>\n<body>\n<h1>{title}</h1>\n"
    );
    let _ = writeln!(
        out,
        "<p class=\"meta\">Updated {} UTC. Recording since {} UTC.</p>",
        utc(now),
        utc(r.since)
    );
    if let Ok(Some(runs)) = runs {
        if let Some(maintenance) = runs["maintenance"].as_str() {
            let _ = writeln!(
                out,
                "<p class=\"banner\">Maintenance: {}</p>",
                esc(maintenance)
            );
        }
        if let Some(pin) = runs["pin"].as_str() {
            let _ = writeln!(out, "<p>Pinned to {}.</p>", esc(pin));
        }
    }
    problems_section(&mut out, r, now, &problems);
    runs_section(&mut out, runs);
    // The tables leave out the tags that no publisher heard in a day, such
    // as ones from before a move. The problems still tell an expected one.
    let columns = day.columns();
    let heard = |id: &str| columns.iter().any(|(_, p)| p.cell(id).is_some());
    let (heard_hour, heard_day) = (hour.with_tags(heard), day.with_tags(heard));
    tags_section(&mut out, r, now, &heard_day);
    coverage_section(&mut out, "last hour", &heard_hour);
    coverage_section(&mut out, "last day", &heard_day);
    boards_section(&mut out, r, now, &hour, &day);
    instances_section(&mut out, r, now, &day);
    out.push_str("</body>\n</html>\n");
    Page {
        html: out,
        problems: problems_json(r, now, &problems),
    }
}

/// The site's problems over the last hour, and the recorder's, worst first
fn problems(r: &Recorder, now: u64, expect: &[(String, String)], hour: &Site) -> Vec<Problem> {
    let mut problems = Vec::new();
    for (topic_name, entry) in r.history.entries() {
        let name = label(topic_name);
        let problem = if r.statuses.get(&entry.client_id).is_some_and(|s| !s.online) {
            Problem::new(
                Grade::Poor,
                format!("offline {topic_name}"),
                format!("{name}: offline"),
            )
        } else if now.saturating_sub(entry.last) > SILENT {
            let silent = time::format(Duration::from_secs(now - entry.last));
            Problem::new(
                Grade::Poor,
                format!("silent {topic_name}"),
                format!("{name}: no diagnostics for {silent}"),
            )
        } else {
            continue;
        };
        problems.push(problem.notify(true).publisher(topic_name));
    }
    let expected: BTreeSet<&String> = expect.iter().map(|(p, _)| p).collect();
    for publisher in expected {
        if !r.history.entries().any(|(name, _)| name == publisher) {
            let text = format!("{}: no diagnostics", label(publisher));
            let problem = Problem::new(Grade::Poor, format!("silent {publisher}"), text);
            problems.push(problem.notify(true).publisher(publisher));
        }
    }
    for (publisher, id) in expect {
        let Some(p) = hour.publisher(publisher) else {
            continue;
        };
        let tag = tag_name(r, id);
        let name = label(publisher);
        let problem = match p.cell(id).map(|cell| cell.grade()) {
            // When no publisher hears it, it is likely the tag's own problem
            None => {
                let elsewhere = hour
                    .columns()
                    .iter()
                    .any(|(other, q)| *other != publisher && q.cell(id).is_some());
                Problem::new(
                    Grade::Poor,
                    format!("missing {publisher} {id}"),
                    format!("{tag}: {name} doesn't hear it"),
                )
                .notify(elsewhere)
            }
            Some(Grade::Poor) => Problem::new(
                Grade::Fair,
                format!("poorly {publisher} {id}"),
                format!("{tag}: {name} hears it poorly"),
            ),
            Some(_) => continue,
        };
        problems.push(problem.publisher(publisher).tag(id));
    }
    problems.extend(coverage(hour, expect));
    for (id, d) in &r.devices {
        if d.battery_low == Some(true) {
            let volts = d.battery.map_or(String::new(), |v| format!(", {v:.2} V"));
            let tag = tag_name(r, id);
            let text = format!("{tag}: low battery{volts}");
            problems.push(Problem::new(Grade::Fair, format!("battery {id}"), text).tag(id));
        }
    }
    problems.sort_by_key(|p| Reverse(p.grade));
    problems
}

/// The site's problems. With expectations, only the expected tags': the
/// others are just in range. A tag that one publisher should hear needs no
/// other.
fn coverage(hour: &Site, expect: &[(String, String)]) -> Vec<Problem> {
    if expect.is_empty() {
        return hour.problems();
    }
    let mut expecters: BTreeMap<&str, usize> = BTreeMap::new();
    for (_, id) in expect.iter().collect::<BTreeSet<_>>() {
        *expecters.entry(id).or_default() += 1;
    }
    hour.with_tags(|id| expecters.contains_key(id))
        .problems()
        .into_iter()
        .filter(|p| {
            let single = p.tag.as_deref().and_then(|id| expecters.get(id)) == Some(&1);
            let others = ["only-one ", "only-instances "];
            !(single && others.iter().any(|kind| p.key.starts_with(kind)))
        })
        .collect()
}

/// Since when each problem has lasted, by its key. One that clears starts
/// over.
fn track(r: &mut Recorder, now: u64, problems: &[Problem]) {
    let keys: BTreeSet<&str> = problems.iter().map(|p| p.key.as_str()).collect();
    r.problems.retain(|key, _| keys.contains(key.as_str()));
    for p in problems {
        r.problems.entry(p.key.clone()).or_insert(now);
    }
}

fn since(r: &Recorder, now: u64, p: &Problem) -> u64 {
    r.problems.get(&p.key).copied().unwrap_or(now)
}

fn problems_section(out: &mut String, r: &Recorder, now: u64, problems: &[Problem]) {
    out.push_str("<h2>Problems</h2>\n");
    if problems.is_empty() {
        out.push_str("<p>None.</p>\n");
        return;
    }
    out.push_str("<ul>\n");
    for p in problems {
        let lasted = time::format(Duration::from_secs(now.saturating_sub(since(r, now, p))));
        let _ = writeln!(
            out,
            "<li>{} {} <small>for {lasted}</small></li>",
            dot(p.grade),
            esc(&p.text)
        );
    }
    out.push_str("</ul>\n");
}

/// `problems.json`: README.md describes it
fn problems_json(r: &Recorder, now: u64, problems: &[Problem]) -> Value {
    let problems: Vec<Value> = problems
        .iter()
        .map(|p| {
            json!({
                "key": p.key,
                "text": p.text,
                "grade": class(p.grade),
                "since": since(r, now, p),
                "notify": p.notify,
                "publisher": p.publisher,
                "tag": p.tag.as_deref().map(mac),
            })
        })
        .collect();
    json!({ "written": now, "problems": problems })
}

/// Without `runs.json`, there's no runner and no section.
fn runs_section(out: &mut String, runs: &Result<Option<Value>, String>) {
    if runs == &Ok(None) {
        return;
    }
    out.push_str("<h2>Runs</h2>\n");
    let runs = match runs {
        Ok(Some(runs)) => runs["runs"].as_array().map_or(&[][..], Vec::as_slice),
        Ok(None) => &[],
        Err(err) => {
            let _ = writeln!(out, "<p>🔴 runs.json: {}</p>", esc(err));
            return;
        }
    };
    if runs.is_empty() {
        out.push_str("<p>No runs yet.</p>\n");
        return;
    }
    out.push_str(
        "<table>\n<tr><th>Started, UTC</th><th>What</th><th>Result</th><th>Summary</th><th></th></tr>\n",
    );
    for run in runs {
        let text = |key: &str| run[key].as_str().unwrap_or_default();
        let started = text("started");
        let started = started.get(..16).unwrap_or(started).replace('T', " ");
        let sha = text("sha");
        let mut what = format!("{} ({})", text("ref"), sha.get(..7).unwrap_or(sha));
        if text("kind") == "check" {
            what.insert_str(0, "daily check of ");
        }
        let result = text("result");
        let dot = match result {
            "success" => "🟢 ",
            "failure" => "🔴 ",
            "running" => "",
            _ => "🟡 ",
        };
        let log = run["log"]
            .as_str()
            .filter(|log| is_log_path(log))
            .map_or(String::new(), |log| {
                format!("<a href=\"{}\">log</a>", esc(log))
            });
        // The test bed's own, which the summary only counts
        let mut faults = String::new();
        for fault in run["faults"].as_array().into_iter().flatten() {
            let fault = esc(fault.as_str().unwrap_or_default());
            let _ = write!(faults, "<br>🟡 {fault}");
        }
        let _ = writeln!(
            out,
            "<tr><td>{}</td><td>{}</td><td>{dot}{}</td>\
             <td class=\"wrap\">{}{faults}</td><td>{log}</td></tr>",
            esc(&started),
            esc(&what),
            esc(result),
            esc(text("summary")),
        );
    }
    out.push_str("</table>\n");
}

/// A log in the page's directory: `runs/<file>`
fn is_log_path(path: &str) -> bool {
    path.strip_prefix("runs/").is_some_and(|file| {
        !file.is_empty()
            && !file.starts_with('.')
            && file
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
    })
}

/// A tag silent for 30 minutes or with a low battery gets a warning
fn tags_section(out: &mut String, r: &Recorder, now: u64, day: &Site) {
    out.push_str("<h2>Tags</h2>\n");
    let rows = day.rows();
    if rows.is_empty() {
        out.push_str("<p>No tags yet.</p>\n");
        return;
    }
    out.push_str("<table>\n<tr><th></th><th>Tag</th><th>Last reading</th><th>Battery</th></tr>\n");
    for (id, name) in rows {
        let device = r.devices.get(id);
        let last = device.and_then(|d| d.readings.back()).map(|(at, _)| *at);
        let low = device.is_some_and(|d| d.battery_low == Some(true));
        let silent = last.is_none_or(|at| now.saturating_sub(at) > 30 * 60);
        let battery = device
            .and_then(|d| d.battery)
            .map_or("-".to_string(), |v| format!("{v:.2} V"));
        let _ = writeln!(
            out,
            "<tr><td>{}</td><td>{}</td><td>{}</td><td>{battery}{}</td></tr>",
            if silent || low { "⚠️" } else { "" },
            esc(name),
            last.map_or("-".to_string(), |at| ago(now, at)),
            if low { " low" } else { "" },
        );
    }
    out.push_str("</table>\n");
}

/// The site table over `over`: the mean of the strongest RSSI per minute or
/// cycle, and the share of minutes or cycles that heard the tag
fn coverage_section(out: &mut String, over: &str, site: &Site) {
    let rows = site.rows();
    let columns = site.columns();
    if rows.is_empty() || columns.is_empty() {
        return;
    }
    let _ = writeln!(
        out,
        "<h2>Coverage, {over} <small>dBm, share of minutes heard</small></h2>"
    );
    out.push_str("<table>\n<tr><th>Tag</th>");
    for (name, _) in &columns {
        let _ = write!(out, "<th>{}</th>", esc(label(name)));
    }
    out.push_str("</tr>\n");
    for (id, name) in rows {
        let _ = write!(out, "<tr><td>{}</td>", esc(name));
        for (_, p) in &columns {
            let _ = write!(out, "<td>{}</td>", cell(p.cell(id)));
        }
        out.push_str("</tr>\n");
    }
    out.push_str("</table>\n");
    let others = site.other_tags();
    if others > 0 {
        let _ = writeln!(
            out,
            "<p class=\"meta\">{} without a discovery config.</p>",
            plural(others, "other tag")
        );
    }
}

/// `🟢 -70 100%`
fn cell(cell: Option<Cell>) -> String {
    cell.map_or("-".to_string(), |cell| {
        let rssi = cell.rssi.map_or("heard".to_string(), |r| r.to_string());
        format!("{} {rssi} {}%", dot(cell.grade()), cell.heard)
    })
}

/// 🔴 when offline or silent
fn state(r: &Recorder, now: u64, client_id: &str, last: u64) -> &'static str {
    let offline = r.statuses.get(client_id).is_some_and(|s| !s.online);
    if offline || now.saturating_sub(last) > SILENT {
        "🔴"
    } else {
        "🟢"
    }
}

fn boards_section(out: &mut String, r: &Recorder, now: u64, hour: &Site, day: &Site) {
    let boards: Vec<_> = day.columns().into_iter().filter(|(_, p)| p.board).collect();
    if boards.is_empty() {
        return;
    }
    out.push_str(
        "<h2>Boards</h2>\n<table>\n<tr><th></th><th>Board</th><th>Wi-Fi</th>\
         <th>Restarts today</th><th>Firmware</th></tr>\n",
    );
    let entries: BTreeMap<&String, _> = r.history.entries().collect();
    for (name, p) in boards {
        let Some(entry) = entries.get(name) else {
            continue;
        };
        let wifi = hour
            .publisher(name)
            .and_then(|h| h.wifi_rssi.get())
            .map_or("-".to_string(), |rssi| {
                format!("{} {rssi} dBm", dot(Grade::wifi(rssi)))
            });
        let board = esc(label(name));
        let board = match r.status_pages.get(name) {
            Some(url) => format!("<a href=\"{}\">{board}</a>", esc(url)),
            None => board,
        };
        let _ = writeln!(
            out,
            "<tr><td>{}</td><td>{board}</td><td>{wifi}</td><td>{}</td><td>{}</td></tr>",
            state(r, now, &entry.client_id, entry.last),
            p.restarts,
            p.firmware.as_deref().map_or("-".to_string(), esc),
        );
    }
    out.push_str("</table>\n");
}

fn instances_section(out: &mut String, r: &Recorder, now: u64, day: &Site) {
    let instances: Vec<_> = day
        .columns()
        .into_iter()
        .filter(|(_, p)| !p.board)
        .collect();
    if instances.is_empty() {
        return;
    }
    out.push_str(
        "<h2>Instances</h2>\n<table>\n<tr><th></th><th>Instance</th><th>Adapter</th>\
         <th>Version</th></tr>\n",
    );
    let entries: BTreeMap<&String, _> = r.history.entries().collect();
    for (name, _) in instances {
        let Some(entry) = entries.get(name) else {
            continue;
        };
        let version = r
            .statuses
            .get(&entry.client_id)
            .and_then(|s| s.version.as_deref());
        let _ = writeln!(
            out,
            "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
            state(r, now, &entry.client_id, entry.last),
            esc(label(name)),
            entry.adapter.as_deref().map_or("-".to_string(), esc),
            version.map_or("-".to_string(), esc),
        );
    }
    out.push_str("</table>\n");
}

fn tag_name(r: &Recorder, id: &str) -> String {
    r.devices
        .get(id)
        .and_then(|d| d.name.clone())
        .unwrap_or_else(|| mac(id))
}

/// `aabbccddee01` to `AA:BB:CC:DD:EE:01`
fn mac(id: &str) -> String {
    let upper = id.to_ascii_uppercase();
    let pairs: Vec<&str> = (0..upper.len())
        .step_by(2)
        .filter_map(|i| upper.get(i..i + 2))
        .collect();
    pairs.join(":")
}

fn dot(grade: Grade) -> &'static str {
    match grade {
        Grade::Good => "🟢",
        Grade::Fair => "🟡",
        Grade::Poor => "🔴",
    }
}

fn class(grade: Grade) -> &'static str {
    match grade {
        Grade::Good => "good",
        Grade::Fair => "fair",
        Grade::Poor => "poor",
    }
}

/// `1:05 ago`
fn ago(now: u64, at: u64) -> String {
    format!(
        "{} ago",
        time::format(Duration::from_secs(now.saturating_sub(at)))
    )
}

/// `2026-11-06 12:34`
fn utc(secs: u64) -> String {
    let (y, m, d) = civil(secs / 86_400);
    let s = secs % 86_400;
    format!("{y}-{m:02}-{d:02} {:02}:{:02}", s / 3600, s / 60 % 60)
}

/// The date of a day since 1970-01-01, by Howard Hinnant's `civil_from_days`
fn civil(days: u64) -> (u64, u64, u64) {
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z % 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + u64::from(m <= 2);
    (y, m, d)
}

/// Escapes text for HTML, also in an attribute
fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::fixtures::{
        HOST1, HOST2, TAG, board_discovery, diagnostics, discovery, instance_diagnostics, online,
        reading,
    };

    const T0: u64 = 1_800_000_000;
    const TAG2: &str = "aabbccddee02";

    fn recorder() -> Recorder {
        let mut r = Recorder::new("ruuvi2mqtt", T0);
        for (id, name) in [(TAG, "Cellar"), (TAG2, "Attic <b>")] {
            r.on_message(
                T0,
                &format!("homeassistant/sensor/ruuvi_{id}/temperature/config"),
                discovery(id, "temperature", name).as_bytes(),
                true,
            );
        }
        for host in [HOST1, HOST2] {
            r.on_message(
                T0,
                &format!("ruuvi2mqtt/status/{host}"),
                online("1.5.2-dev (6534f11)").as_bytes(),
                true,
            );
        }
        for n in 0..10 {
            let at = T0 + n * 60;
            let tags = format!(r#"{{"{TAG}":{{"rssi":-70,"readings":1}}}}"#);
            r.on_message(
                at,
                &format!("ruuvi2mqtt/diagnostics/{HOST1}"),
                instance_diagnostics(HOST1, &tags).as_bytes(),
                false,
            );
            r.on_message(
                at,
                &format!("ruuvi2mqtt/diagnostics/{HOST2}"),
                instance_diagnostics(HOST2, "{}").as_bytes(),
                false,
            );
            r.on_message(
                at,
                &format!("ruuvi2mqtt/{TAG}"),
                reading(HOST1).as_bytes(),
                false,
            );
        }
        r
    }

    /// The marks, the texts, and `*` for those that notify
    fn texts(problems: &[Problem]) -> Vec<String> {
        problems
            .iter()
            .map(|p| {
                let notify = if p.notify { "*" } else { "" };
                format!("{}{}{notify}", p.grade.mark(), p.text)
            })
            .collect()
    }

    #[test]
    fn expected_tags_and_silent_publishers_are_problems() {
        let mut r = recorder();
        let low = reading(HOST1)
            .replace(r#""battery":2.9"#, r#""battery":2.1"#)
            .replace(r#""battery_low":false"#, r#""battery_low":true"#);
        r.on_message(
            T0 + 600,
            &format!("ruuvi2mqtt/{TAG}"),
            low.as_bytes(),
            false,
        );
        let expect = [
            (HOST1.to_string(), TAG.to_string()),
            (HOST2.to_string(), TAG.to_string()),
            (HOST1.to_string(), TAG2.to_string()),
            ("ruuvi2mqtt_host3".to_string(), TAG.to_string()),
        ];
        let now = T0 + 600;
        let site = Site::from_parts(
            [(TAG.to_string(), "Cellar".to_string())].into(),
            r.history.window(now, HOUR),
        );
        assert_eq!(
            texts(&problems(&r, now, &expect, &site)),
            [
                "!host3: no diagnostics*",
                "!Cellar: host2 doesn't hear it*",
                // No publisher hears it: the tag's own alerts tell
                "!Attic <b>: host1 doesn't hear it",
                "~Cellar: only host1 hears it well",
                "~Cellar: low battery, 2.10 V",
            ]
        );
        // Ten minutes later, without diagnostics
        r.on_message(
            now,
            &format!("ruuvi2mqtt/status/{HOST2}"),
            br#"{"state":"offline"}"#,
            true,
        );
        let later = now + 600;
        let lines = texts(&problems(&r, later, &[], &site));
        assert_eq!(
            lines[..2],
            ["!host1: no diagnostics for 11:00*", "!host2: offline*"]
        );
    }

    #[test]
    fn expectations_limit_the_coverage_problems() {
        let r = recorder();
        let now = T0 + 600;
        let hour = Site::from_parts(
            [
                (TAG.to_string(), "Cellar".to_string()),
                (TAG2.to_string(), "Attic".to_string()),
            ]
            .into(),
            r.history.window(now, HOUR),
        );
        // Without them, every tag
        assert_eq!(
            texts(&coverage(&hour, &[])),
            ["!not heard: Attic", "~Cellar: only host1 hears it well"]
        );
        // Only host1 should hear Cellar, and Attic isn't the test bed's
        let expect = [(HOST1.to_string(), TAG.to_string())];
        assert!(coverage(&hour, &expect).is_empty());
        // Both should
        let expect = [
            (HOST1.to_string(), TAG.to_string()),
            (HOST2.to_string(), TAG.to_string()),
        ];
        assert_eq!(
            texts(&coverage(&hour, &expect)),
            ["~Cellar: only host1 hears it well"]
        );
        // With a board, the same for a spot for a board
        let mut r = r;
        r.on_message(
            now,
            "ruuvi2mqtt/diagnostics/c6",
            diagnostics("1.5.2", 3, "{}").as_bytes(),
            false,
        );
        let hour = Site::from_parts(
            [(TAG.to_string(), "Cellar".to_string())].into(),
            r.history.window(now, HOUR),
        );
        assert_eq!(
            texts(&coverage(&hour, &[])),
            ["~Cellar: only instances hear it well, a spot for a board"]
        );
        let expect = [(HOST1.to_string(), TAG.to_string())];
        assert!(coverage(&hour, &expect).is_empty());
    }

    #[test]
    fn renders_the_page() {
        let mut r = recorder();
        let runs = json!({
            "maintenance": "moving <222>",
            "pin": "my-branch",
            "runs": [
                {"kind": "check", "ref": "main", "sha": "6534f11d0c2e", "result": "success",
                 "summary": "6 tags, test bed faults: 2", "started": "2026-11-06T02:00:01Z",
                 "faults": ["Cellar silent for 15 minutes", "<x> restarted"],
                 "log": "runs/20261106T020001Z-canary.log"},
                {"kind": "run", "ref": "my-branch", "sha": "1234567890ab", "result": "failure",
                 "summary": "a <script>", "started": "2026-11-05T21:00:00Z",
                 "log": "../etc/passwd"},
            ],
        });
        let page = r.page(T0 + 600, "Test bed", &[], &Ok(Some(runs))).html;
        for s in [
            "<title>Test bed</title>",
            "Updated 2027-01-15 08:10 UTC. Recording since 2027-01-15 08:00 UTC.",
            "<p class=\"banner\">Maintenance: moving &lt;222&gt;</p>",
            "<p>Pinned to my-branch.</p>",
            "<li>🟡 Cellar: only host1 hears it well <small>for 0:00</small></li>",
            "<li>🔴 not heard: Attic &lt;b&gt; <small>for 0:00</small></li>",
            "<td>2026-11-06 02:00</td><td>daily check of main (6534f11)</td><td>🟢 success</td>",
            "<td class=\"wrap\">6 tags, test bed faults: 2<br>🟡 Cellar silent for 15 minutes\
             <br>🟡 &lt;x&gt; restarted</td>",
            "<a href=\"runs/20261106T020001Z-canary.log\">log</a>",
            "<td>🔴 failure</td><td class=\"wrap\">a &lt;script&gt;</td><td></td>",
            "<tr><td></td><td>Cellar</td><td>1:00 ago</td><td>2.90 V</td></tr>",
            "<h2>Coverage, last hour <small>dBm, share of minutes heard</small></h2>",
            "<tr><th>Tag</th><th>host1</th><th>host2</th></tr>",
            "<tr><td>Cellar</td><td>🟢 -70 100%</td><td>-</td></tr>",
            "<h2>Coverage, last day",
            "<tr><td>🟢</td><td>host1</td><td>hci0: Example BT Adapter (1234:5678)</td>\
             <td>1.5.2-dev (6534f11)</td></tr>",
        ] {
            assert!(page.contains(s), "{s:?} in:\n{page}");
        }
        assert!(!page.contains("passwd"));
        assert!(!page.contains("<script>"));
        // Not heard in a day: a problem, but in no table
        assert!(!page.contains("<td>Attic &lt;b&gt;</td>"));
    }

    #[test]
    fn renders_the_boards() {
        let mut r = Recorder::new("ruuvi2mqtt", T0);
        for (at, cycle) in [(T0, 5), (T0 + 60, 1)] {
            r.on_message(
                at,
                "ruuvi2mqtt/diagnostics/c6-board",
                diagnostics("1.5.2", cycle, "{}").as_bytes(),
                false,
            );
        }
        let page = r.page(T0 + 600, "Test bed", &[], &Ok(None)).html;
        let row = "<tr><td>🔴</td><td>c6-board</td><td>🟢 -61 dBm</td><td>1</td>\
                   <td>ruuvi2mqtt-esp32 1.5.2</td></tr>";
        assert!(page.contains(row), "{row:?} in:\n{page}");
        assert!(!page.contains("<h2>Instances</h2>"));
        // Linked only while its config has a page
        let topic = "homeassistant/sensor/ruuvi2mqtt-c6/scan_mode/config";
        let continuous = board_discovery().replace(
            r#""device":{"#,
            r#""device":{"configuration_url":"http://ruuvi2mqtt-c6.local/","#,
        );
        r.on_message(T0 + 600, topic, continuous.as_bytes(), true);
        let page = r.page(T0 + 600, "Test bed", &[], &Ok(None)).html;
        let link = "<td><a href=\"http://ruuvi2mqtt-c6.local/\">c6-board</a></td>";
        assert!(page.contains(link), "{link:?} in:\n{page}");
        r.on_message(T0 + 600, topic, board_discovery().as_bytes(), true);
        let page = r.page(T0 + 600, "Test bed", &[], &Ok(None)).html;
        assert!(page.contains("<td>c6-board</td>"));
    }

    #[test]
    fn renders_without_runs_or_with_a_broken_file() {
        let mut r = Recorder::new("ruuvi2mqtt", T0);
        let page = r.page(T0, "Dev & boards", &[], &Ok(None)).html;
        assert!(page.contains("<title>Dev &amp; boards</title>"));
        assert!(page.contains("<h1>Dev &amp; boards</h1>"));
        assert!(page.contains("<p>None.</p>"));
        assert!(!page.contains("<h2>Runs</h2>"));
        assert!(page.contains("<p>No tags yet.</p>"));
        let page = r
            .page(T0, "Test bed", &[], &Ok(Some(json!({"runs": []}))))
            .html;
        assert!(page.contains("<p>No runs yet.</p>"));
        let page = r.page(T0, "Test bed", &[], &Err("not JSON".into())).html;
        assert!(page.contains("<p>🔴 runs.json: not JSON</p>"));
    }

    #[test]
    fn problems_keep_their_start_across_a_restart() {
        let mut r = recorder();
        let expect = [(HOST2.to_string(), TAG.to_string())];
        r.page(T0 + 540, "Test bed", &expect, &Ok(None));
        let state = r.state(T0 + 600).to_string();
        let (mut r, _) = Recorder::load("ruuvi2mqtt", &state).unwrap();
        let problems = r.page(T0 + 800, "Test bed", &expect, &Ok(None)).problems;
        assert_eq!(problems["written"], T0 + 800);
        assert_eq!(
            problems["problems"][0],
            json!({
                "key": format!("missing {HOST2} {TAG}"),
                "text": "Cellar: host2 doesn't hear it",
                "grade": "poor",
                "since": T0 + 540,
                "notify": true,
                "publisher": HOST2,
                "tag": "AA:BB:CC:DD:EE:01",
            })
        );
        // Diagnostics stopped: a new problem, and the old one still lasts
        let problems = r.page(T0 + 1200, "Test bed", &expect, &Ok(None)).problems;
        let since: Vec<_> = problems["problems"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| (p["key"].as_str().unwrap(), p["since"].as_u64().unwrap()))
            .collect();
        assert!(since.contains(&(&format!("silent {HOST1}")[..], T0 + 1200)));
        assert!(since.contains(&(&format!("missing {HOST2} {TAG}")[..], T0 + 540)));
        // Cleared, then back: it starts over
        r.page(T0 + 1260, "Test bed", &[], &Ok(None));
        let problems = r.page(T0 + 1320, "Test bed", &expect, &Ok(None)).problems;
        let missing = problems["problems"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["key"] == format!("missing {HOST2} {TAG}"));
        assert_eq!(missing.map(|p| &p["since"]), Some(&json!(T0 + 1320)));
    }

    #[test]
    fn log_paths_stay_in_runs() {
        assert!(is_log_path("runs/20261106T020001Z-canary.log"));
        for path in [
            "runs/",
            "runs/../x",
            "runs/a/b",
            "/etc/passwd",
            "runs/.hidden",
            "x.log",
        ] {
            assert!(!is_log_path(path), "{path}");
        }
    }

    #[test]
    fn formats_dates_and_macs() {
        assert_eq!(utc(0), "1970-01-01 00:00");
        assert_eq!(utc(951_782_400), "2000-02-29 00:00");
        assert_eq!(utc(1_793_958_300), "2026-11-06 09:45");
        assert_eq!(mac("aabbccddee01"), "AA:BB:CC:DD:EE:01");
    }
}
