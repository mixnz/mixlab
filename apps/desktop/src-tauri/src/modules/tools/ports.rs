//! Reads the output of `netstat`, `ss` and `lsof` into a list of listening ports.
//!
//! There is no `Command` in this file, and that is deliberate: running commands depends on the
//! machine, depends on permissions, and cannot be tested in CI. Reading output is a pure function —
//! and it is where every bug will be, because three platforms have three formats, each with its own
//! odd lines. `commands.rs` keeps the other half.
//!
//! `allow(dead_code)` for the whole file, and it is what makes the split above work: `collect()` in
//! `commands.rs` is `cfg`-gated per platform, so on any machine it only calls **one** of the three
//! parsers, while all three are deliberately compiled everywhere so that **all three's tests run
//! everywhere**. Without this line clippy reports the other two parsers as dead code on Windows,
//! and reports `parse_netstat` as dead code on CI's Linux machine. Fencing it in exactly this file
//! is acceptable because there is nothing in here but the three parsers and their helpers.
#![allow(dead_code)]

use serde::Serialize;
use std::collections::HashMap;

/// A port being listened on on this machine.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ListeningPort {
    pub port: u16,
    /// The listening address: `0.0.0.0`, `127.0.0.1`, `::`. Tells "open to the outside" apart from
    /// "localhost only".
    pub address: String,
    pub pid: u32,
    /// `None` when the port could be found but not the process name — usually a lack of
    /// permissions.
    pub process: Option<String>,
}

/// Splits `0.0.0.0:445` or `[::]:445` into address and port.
///
/// Splits at the **last** colon: an IPv6 address is full of colons inside it, so splitting at the
/// first gives `[` and a pile of garbage.
fn split_address(text: &str) -> Option<(String, u16)> {
    let cut = text.rfind(':')?;
    let (host, port) = text.split_at(cut);
    let port: u16 = port[1..].parse().ok()?;
    let host = host.trim_start_matches('[').trim_end_matches(']');
    Some((host.to_string(), port))
}

/// The PID → process name table, read from `tasklist /FO CSV /NH`.
///
/// Each line is `"name","pid","session","number","memory"`. Split on `","` rather than on a single
/// comma: a process name may contain commas, and that is exactly why it sits in quotes.
fn tasklist_names(text: &str) -> HashMap<u32, String> {
    let mut names = HashMap::new();
    for line in text.lines() {
        let line = line.trim();
        if !line.starts_with('"') {
            continue;
        }
        let mut parts = line.trim_matches('"').split("\",\"");
        let Some(name) = parts.next() else { continue };
        let Some(pid) = parts.next().and_then(|p| p.trim().parse::<u32>().ok()) else {
            continue;
        };
        names.insert(pid, name.to_string());
    }
    names
}

/// Reads `netstat -ano` plus `tasklist /FO CSV /NH`.
///
/// **`netstat -ano`, not `netstat -ano -p TCP`**: the `-p TCP` flag filters out all of IPv6, so a
/// service listening only on `[::]` would vanish from the table without a trace.
///
/// Without `-p` the output also includes UDP, and the way to tell them apart is the **number of
/// columns**: a TCP line has 5 columns because it has a state column, a UDP line only 4. So the
/// rule here is exactly 5 columns, first column `TCP`, fourth column `LISTENING` — UDP drops out
/// for lacking a column, `ESTABLISHED` TCP drops out for the wrong state.
pub fn parse_netstat(netstat: &str, tasklist: &str) -> Vec<ListeningPort> {
    let names = tasklist_names(tasklist);
    let mut ports = Vec::new();
    for line in netstat.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() != 5 || fields[0] != "TCP" || fields[3] != "LISTENING" {
            continue;
        }
        let Some((address, port)) = split_address(fields[1]) else {
            continue;
        };
        let Ok(pid) = fields[4].parse::<u32>() else {
            continue;
        };
        ports.push(ListeningPort {
            port,
            address,
            pid,
            process: names.get(&pid).cloned(),
        });
    }
    ports
}

/// Pulls the first name and PID out of `users:(("nginx",pid=123,fd=6),("nginx",pid=124,fd=6))`.
fn parse_ss_users(field: &str) -> Option<(String, u32)> {
    let rest = field.strip_prefix("users:((")?;
    let name = rest.strip_prefix('"')?;
    let end = name.find('"')?;
    let name = &name[..end];
    let pid_at = rest.find("pid=")? + 4;
    let pid: String = rest[pid_at..]
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    Some((name.to_string(), pid.parse().ok()?))
}

/// Reads `ss -lntp`.
///
/// The `-H` flag to drop the header is not used: it only exists in newer `iproute2`, and
/// recognising the header line here is cheaper than demanding a version. The hard part is
/// `users:(("nginx",pid=123,fd=6))` — the name and PID are nested in parentheses, and a port may be
/// held by several processes at once.
pub fn parse_ss(text: &str) -> Vec<ListeningPort> {
    let mut ports = Vec::new();
    for line in text.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        // `State Recv-Q Send-Q Local:Port Peer:Port [Process]`
        if fields.len() < 5 || fields[0] != "LISTEN" {
            continue;
        }
        let Some((address, port)) = split_address(fields[3]) else {
            continue;
        };
        let (process, pid) = match fields.get(5).and_then(|f| parse_ss_users(f)) {
            Some((name, pid)) => (Some(name), pid),
            // No permission to see processes: the port is still a useful answer.
            None => (None, 0),
        };
        ports.push(ListeningPort {
            port,
            address,
            pid,
            process,
        });
    }
    ports
}

/// Reads `lsof -nP -iTCP -sTCP:LISTEN -Fpcn`.
///
/// Field format: one field per line, the first character is the field name. `p` starts a process,
/// `c` is its command name, and every `n` line after that belongs to the latest process — so the
/// open `p` and `c` have to be remembered rather than reading each line independently.
pub fn parse_lsof(text: &str) -> Vec<ListeningPort> {
    let mut ports = Vec::new();
    let mut pid = 0u32;
    let mut command: Option<String> = None;
    for line in text.lines() {
        let Some(tag) = line.chars().next() else {
            continue;
        };
        let value = &line[1..];
        match tag {
            'p' => {
                pid = value.parse().unwrap_or(0);
                command = None;
            }
            'c' => command = Some(value.to_string()),
            'n' => {
                // `*:3000` means listening on every address.
                let text = value.replace("*:", "0.0.0.0:");
                if let Some((address, port)) = split_address(&text) {
                    ports.push(ListeningPort {
                        port,
                        address,
                        pid,
                        process: command.clone(),
                    });
                }
            }
            _ => {}
        }
    }
    ports
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Captured verbatim from `netstat -ano` on Windows 11.
    const NETSTAT: &str = "\
Active Connections

  Proto  Local Address          Foreign Address        State           PID
  TCP    0.0.0.0:135            0.0.0.0:0              LISTENING       1588
  TCP    0.0.0.0:445            0.0.0.0:0              LISTENING       4
  TCP    127.0.0.1:65370        127.0.0.1:65371        ESTABLISHED     21928
  TCP    [::]:135               [::]:0                 LISTENING       1588
  UDP    0.0.0.0:53             *:*                                    4864
";

    const TASKLIST: &str = "\
\"System Idle Process\",\"0\",\"Services\",\"0\",\"8 K\"
\"System\",\"4\",\"Services\",\"0\",\"5.844 K\"
\"svchost.exe\",\"1588\",\"Services\",\"0\",\"12.000 K\"
";

    #[test]
    fn netstat_chi_lay_dong_dang_nghe() {
        let ports = parse_netstat(NETSTAT, TASKLIST);
        // Two IPv4 LISTENING lines plus one IPv6 line; ESTABLISHED and UDP must not get in.
        assert_eq!(ports.len(), 3);
        assert!(ports.iter().all(|p| p.port != 65370));
        assert!(ports.iter().all(|p| p.port != 53));
    }

    #[test]
    fn netstat_doc_duoc_ipv6() {
        let ports = parse_netstat(NETSTAT, TASKLIST);
        let v6 = ports.iter().find(|p| p.address == "::").unwrap();
        assert_eq!(v6.port, 135);
        assert_eq!(v6.pid, 1588);
    }

    #[test]
    fn netstat_tra_ten_tien_trinh_theo_pid() {
        let ports = parse_netstat(NETSTAT, TASKLIST);
        let p135 = ports.iter().find(|p| p.port == 135).unwrap();
        assert_eq!(p135.process.as_deref(), Some("svchost.exe"));
        let p445 = ports.iter().find(|p| p.port == 445).unwrap();
        assert_eq!(p445.process.as_deref(), Some("System"));
    }

    #[test]
    fn netstat_de_none_khi_khong_co_pid_trong_tasklist() {
        let netstat =
            "  TCP    0.0.0.0:9999           0.0.0.0:0              LISTENING       777\n";
        let ports = parse_netstat(netstat, TASKLIST);
        assert_eq!(ports[0].process, None);
    }

    #[test]
    fn tasklist_khong_vo_vi_ten_co_dau_phay() {
        let names = tasklist_names("\"a,b.exe\",\"42\",\"Services\",\"0\",\"1 K\"\n");
        assert_eq!(names.get(&42).map(String::as_str), Some("a,b.exe"));
    }

    const SS: &str = "\
State      Recv-Q Send-Q Local Address:Port  Peer Address:Port Process
LISTEN     0      511          0.0.0.0:80          0.0.0.0:*    users:((\"nginx\",pid=123,fd=6))
LISTEN     0      4096            [::]:5432             [::]:*    users:((\"postgres\",pid=99,fd=7))
LISTEN     0      128          0.0.0.0:22          0.0.0.0:*
";

    #[test]
    fn ss_doc_ten_va_pid_trong_ngoac() {
        let ports = parse_ss(SS);
        let nginx = ports.iter().find(|p| p.port == 80).unwrap();
        assert_eq!(nginx.process.as_deref(), Some("nginx"));
        assert_eq!(nginx.pid, 123);
    }

    #[test]
    fn ss_bo_dong_tieu_de() {
        assert_eq!(parse_ss(SS).len(), 3);
    }

    #[test]
    fn ss_doc_duoc_ipv6() {
        let pg = parse_ss(SS).into_iter().find(|p| p.port == 5432).unwrap();
        assert_eq!(pg.address, "::");
        assert_eq!(pg.pid, 99);
    }

    // Without permission `ss` prints no process column. The port must still show up.
    #[test]
    fn ss_van_tra_cong_khi_khong_co_cot_tien_trinh() {
        let ssh = parse_ss(SS).into_iter().find(|p| p.port == 22).unwrap();
        assert_eq!(ssh.process, None);
        assert_eq!(ssh.pid, 0);
    }

    const LSOF: &str = "\
p123
cnode
n*:3000
n127.0.0.1:9229
p456
cpostgres
n[::1]:5432
";

    #[test]
    fn lsof_gan_moi_dong_n_vao_tien_trinh_gan_nhat() {
        let ports = parse_lsof(LSOF);
        assert_eq!(ports.len(), 3);
        assert_eq!(ports[0].pid, 123);
        assert_eq!(ports[0].process.as_deref(), Some("node"));
        assert_eq!(ports[1].pid, 123);
        assert_eq!(ports[2].pid, 456);
        assert_eq!(ports[2].process.as_deref(), Some("postgres"));
    }

    #[test]
    fn lsof_doi_sao_thanh_moi_dia_chi() {
        let ports = parse_lsof(LSOF);
        assert_eq!(ports[0].address, "0.0.0.0");
        assert_eq!(ports[0].port, 3000);
    }

    #[test]
    fn lsof_doc_duoc_ipv6() {
        let ports = parse_lsof(LSOF);
        assert_eq!(ports[2].address, "::1");
        assert_eq!(ports[2].port, 5432);
    }

    #[test]
    fn cat_dia_chi_o_dau_hai_cham_cuoi_cung() {
        assert_eq!(split_address("[::]:445"), Some(("::".to_string(), 445)));
        assert_eq!(
            split_address("127.0.0.1:8080"),
            Some(("127.0.0.1".to_string(), 8080))
        );
        assert_eq!(split_address("khong-co-cong"), None);
    }
}
