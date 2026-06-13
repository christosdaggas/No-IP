use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use clap::{Parser, ValueEnum};
use log::debug;

use noip_duc::{noip2, public_ip::IpMethods, updater, NotificationLogger, SleepOnlyController};

// Used to handle --import without requiring --username/--password.
#[derive(Debug, Parser)]
struct PreConfig {
    /// Import config from noip2 and display it as environment variables.
    #[arg(long, num_args = 0..=1, default_missing_value = "/etc/no-ip2.conf")]
    import: PathBuf,
}

#[derive(Parser)]
#[command(about = "No-IP Dynamic Update Client", version)]
struct Config {
    /// Your www.noip.com username. For better security, use Update Group credentials. https://www.noip.com/members/dns/dyn-groups.php
    #[arg(short, long, env = "NOIP_USERNAME")]
    username: String,

    /// Your www.noip.com password. For better security, use Update Group credentials. https://www.noip.com/members/dns/dyn-groups.php
    #[arg(short, long, env = "NOIP_PASSWORD")]
    password: String,

    /// Comma separated list of groups and hostnames to update.
    #[arg(short = 'g', long, env = "NOIP_HOSTNAMES", value_parser = parse_hostnames)]
    hostnames: Option<std::vec::Vec<String>>,

    /// How often to check for a new IP address. Minimum: every 2 minutes.
    #[arg(long, env = "NOIP_CHECK_INTERVAL", default_value = "5m", value_parser = parse_duration_arg)]
    check_interval: Duration,

    /// Timeout when making HTTP requests.
    #[arg(long, env = "NOIP_HTTP_TIMEOUT", default_value = "10s", value_parser = parse_duration_arg)]
    http_timeout: Duration,

    #[cfg(target_family = "unix")]
    /// Fork into the background
    #[arg(long)]
    daemonize: bool,

    #[cfg(target_family = "unix")]
    /// When daemonizing, become this user.
    #[arg(long, env = "NOIP_DAEMON_USER")]
    daemon_user: Option<String>,

    #[cfg(target_family = "unix")]
    /// When daemonizing, become this group.
    #[arg(long, env = "NOIP_DAEMON_GROUP")]
    daemon_group: Option<String>,

    #[cfg(target_family = "unix")]
    /// When daemonizing, write process id to this file.
    #[arg(long, env = "NOIP_DAEMON_PID_FILE")]
    daemon_pid_file: Option<PathBuf>,

    /// Increase logging verbosity. May be used multiple times.
    #[arg(short, long, action = clap::ArgAction::Count)]
    verbose: u8,

    /// Set the log level. Possible values: trace, debug, info, warn, error, critical. Overrides --verbose.
    #[arg(short, long, env = "NOIP_LOG_LEVEL", value_enum, ignore_case = true)]
    log_level: Option<LogLevel>,

    /// Command to run when the IP address changes. It is run with the environment variables
    /// CURRENT_IP and LAST_IP set. Also, {{CURRENT_IP}} and {{LAST_IP}} are replaced with the
    /// respective values. This allows you to provide the variables as arguments to your command or
    /// read them from the environment. The command is always executed in a shell, sh or cmd on
    /// Windows.
    ///
    /// Example
    ///
    ///   noip_duc -e 'mail -s "IP changed to {{CURRENT_IP}} from {{LAST_IP}}" user@example.com'
    #[arg(short = 'e', long, env = "NOIP_EXEC_ON_CHANGE")]
    exec_on_change: Option<String>,

    /// Methods used to discover the public IP, as a comma separated list. They are tried in order
    /// until a public IP is found. Failed methods are not retried unless all methods fail.
    ///
    /// Possible values are
    /// - 'aws-metadata': uses the AWS metadata URL to get the Elastic IP
    ///                   associated with your instance.
    /// - 'dns': Use No-IP's DNS public IP lookup system.
    /// - 'dns:<nameserver>:<port>:<qname>:<record type>': custom DNS lookup.
    /// - 'http': No-IP's HTTP method on port 80.
    /// - 'http-port-8245': No-IP's HTTP method on port 8245.
    /// - 'static:<ip address>': always use this IP address. Helpful with --once.
    /// - HTTP URL: An HTTP URL that returns only an IP address.
    #[arg(
        long,
        env = "NOIP_IP_METHOD",
        default_value = "dns,http,http-port-8245",
        verbatim_doc_comment
    )]
    ip_method: String,

    /// Find the public IP and send an update, then exit. This is a good method to verify correct
    /// credentials.
    #[arg(long)]
    once: bool,
}

struct ResolvedConfig {
    raw: Config,
    ip_method: IpMethods,
}

// Manual Debug impl that redacts the password so `debug!("{:?}", config)` (and any
// other Debug printing) cannot leak credentials into logs.
impl fmt::Debug for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut d = f.debug_struct("Config");
        d.field("username", &self.username);
        d.field("password", &"<redacted>");
        d.field("hostnames", &self.hostnames);
        d.field("check_interval", &self.check_interval);
        d.field("http_timeout", &self.http_timeout);
        #[cfg(target_family = "unix")]
        {
            d.field("daemonize", &self.daemonize);
            d.field("daemon_user", &self.daemon_user);
            d.field("daemon_group", &self.daemon_group);
            d.field("daemon_pid_file", &self.daemon_pid_file);
        }
        d.field("verbose", &self.verbose);
        d.field("log_level", &self.log_level);
        d.field("exec_on_change", &self.exec_on_change);
        d.field("ip_method", &self.ip_method);
        d.field("once", &self.once);
        d.finish()
    }
}

impl<'a> From<&'a ResolvedConfig> for noip_duc::Config<'a> {
    fn from(config: &'a ResolvedConfig) -> Self {
        Self {
            username: config.raw.username.as_str(),
            password: config.raw.password.as_str(),
            hostnames: config.raw.hostnames.as_ref(),
            check_interval: config.raw.check_interval,
            http_timeout: config.raw.http_timeout,
            exec_on_change: config.raw.exec_on_change.as_deref(),
            ip_method: &config.ip_method,
            once: config.raw.once,
        }
    }
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum LogLevel {
    Trace,
    Debug,
    Info,
    #[value(alias = "warning")]
    Warn,
    Error,
    Critical,
}

use std::fmt;
impl fmt::Display for LogLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use LogLevel::*;
        match self {
            Trace => f.write_str("trace"),
            Debug => f.write_str("debug"),
            Info => f.write_str("info"),
            Warn => f.write_str("warn"),
            Error => f.write_str("error"),
            Critical => f.write_str("critical"),
        }
    }
}

fn parse_duration_arg(s: &str) -> Result<Duration, String> {
    humantime::parse_duration(s).map_err(|e| e.to_string())
}

// May be hostnames or group names
fn parse_hostnames(s: &str) -> Result<Vec<String>, String> {
    if s.len() >= 4000 {
        return Err("hostnames too long".into());
    }

    let hostnames: Vec<String> = s.split(',').map(|s| s.trim().to_owned()).collect();

    for h in &hostnames {
        // Group names are alphanumeric only
        if h.chars().all(|c| char::is_ascii_alphanumeric(&c)) {
            continue;
        }
        if !is_hostname(h) {
            return Err(format!(
                "invalid hostname {h}. Hostnames must be a comma separated list of hostnames and group names."
            ));
        }
    }

    Ok(hostnames)
}

fn is_hostname(h: &str) -> bool {
    // May contain a round-robin label
    let h = match h.split_once('@') {
        Some((h, rr)) => {
            if !is_rr_label(rr) {
                return false;
            }
            h
        }
        None => h,
    };

    if h.split('.').count() > 63 {
        return false;
    }

    h.split('.').all(is_label)
}

// Must be all alphanumeric or hyphen. Since these will always be A or AAAA they cannot
// start with `_` like TXT or SRV can.
fn is_label(s: &str) -> bool {
    s.chars().all(|c| char::is_ascii_alphanumeric(&c) || c == '-')
        // Cannot start with hyphen or be empty
        && s.chars().next().map_or(false, |c| c != '-')
        // Cannot end with hyphen or be empty
        && s.chars().last().map_or(false, |c| c != '-')
}

// Check round-robin label. It is the part after an @ in the hostname field.
fn is_rr_label(s: &str) -> bool {
    s.chars().all(|c| char::is_ascii_alphanumeric(&c) || matches!(c, '-' | '_'))
        // Cannot start with hyphen or be empty
        && s.chars().next().map_or(false, |c| c != '-')
}

fn main() -> anyhow::Result<()> {
    // Handle --import first to avoid required --username and --password
    if let Ok(c) = PreConfig::try_parse() {
        let imported = noip2::import(&c.import)?;
        print!("{}", imported);
        return Ok(());
    };

    let raw = Config::parse();

    if raw.check_interval < Duration::from_secs(120) {
        anyhow::bail!("--check_interval must be no less than 2 minutes");
    }

    let ip_method: IpMethods = raw
        .ip_method
        .parse()
        .map_err(|e: noip_duc::public_ip::ParseError| {
            anyhow::anyhow!("invalid --ip-method: {e}")
        })?;

    let config = ResolvedConfig { raw, ip_method };

    let log_level = config.raw.log_level.as_ref().unwrap_or(match config.raw.verbose {
        0 => &LogLevel::Info,
        1 => &LogLevel::Debug,
        _ => &LogLevel::Trace,
    });

    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or(log_level.to_string()),
    )
    .init();

    #[cfg(target_family = "unix")]
    if config.raw.daemonize {
        daemonize(&config.raw)?;
    }

    debug!("{:?}", config.raw);

    updater(
        (&config).into(),
        NotificationLogger {},
        SleepOnlyController {},
    )
    .map_err(Into::into)
}

#[cfg(target_family = "unix")]
fn daemonize(c: &Config) -> Result<()> {
    use daemonize::Daemonize;

    let mut daemonize = Daemonize::new().working_directory("/");

    if let Some(user) = &c.daemon_user {
        daemonize = match user.parse::<u32>() {
            Err(_) => daemonize.user(user.as_str()),
            Ok(uid) => daemonize.user(uid),
        }
    }

    if let Some(group) = &c.daemon_group {
        daemonize = match group.parse::<u32>() {
            Err(_) => daemonize.group(group.as_str()),
            Ok(gid) => daemonize.group(gid),
        }
    }

    if let Some(pid_file) = &c.daemon_pid_file {
        daemonize = daemonize.pid_file(pid_file).chown_pid_file(true);
    }

    daemonize.start()?;

    log::info!("running in background");

    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn is_rr_label_good() {
        for s in ["SERVER-1", "SERVER_1", "_TEST", "_test", "test-"] {
            assert!(is_rr_label(s), r#"input="{s}""#);
        }
    }

    #[test]
    fn is_rr_label_bad() {
        for s in ["SERVER 1", "-test", "^TEST", "te&st", "te|t"] {
            assert!(!is_rr_label(s), r#"input="{s}""#);
        }
    }

    #[test]
    fn is_hostname_good() {
        for s in ["h", "h.test", "h.example.com", "h.example.com@test"] {
            assert!(is_hostname(s), r#"input="{s}""#);
        }
    }

    #[test]
    fn is_hostname_bad() {
        for s in [
            " ",
            "h test",
            "h.example com",
            "h.example.com@-test",
            "h.example.com^test",
        ] {
            assert!(!is_hostname(s), r#"input="{s}""#);
        }
    }
}
