use std::fmt;
use std::net::IpAddr;
use std::time::Duration;

use log::debug;
use percent_encoding::{utf8_percent_encode, AsciiSet, CONTROLS};

const UPDATE_URL: &str = "https://dynupdate.no-ip.com/nic/update";

// https://url.spec.whatwg.org/#query-percent-encode-set
const QUERY_SET: &AsciiSet = &CONTROLS.add(b' ').add(b'"').add(b'#').add(b'<').add(b'>');

type Changed = bool;
type UpdateResult = std::result::Result<Changed, UpdateError>;

#[derive(Clone, Debug)]
pub enum UpdateError {
    NoHost,
    BadAuth,
    BadAgent,
    NotDonator,
    Abuse,
    NineOneOne,
    Unknown(String),
    StatusCode(i32, String),
    Connection(String),
}

impl fmt::Display for UpdateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use UpdateError::*;
        match self {
            NoHost => f.write_str("No host or group was specified. Please create a group with a password to update at https://my.noip.com/dynamic-dns/groups"),
            BadAuth => f.write_str("Incorrect credentials"),
            BadAgent => f.write_str("Client disabled, client must not perform further updates"),
            NotDonator => f.write_str("This feature is not available for you account"),
            Abuse => f.write_str("Client rejected due to abuse"),
            NineOneOne => f.write_str("System outage, please wait longer than usual to try again"),
            Unknown(msg) => write!(f, "unknown error, received '{}'", msg),
            StatusCode(code, reason) => write!(f, "HTTP error {} {}", code, reason),
            Connection(msg) => write!(f, "Connection failed, {}", msg),
        }
    }
}

impl std::error::Error for UpdateError {}

// Can't use u64::MAX here, it'll panic :). Let's give the user an occasional reminder.
const FOREVER: u64 = 14 * 24 * 60 * 60;

impl UpdateError {
    // Cause the retry interval to jump to the max when we receive a "disable" type response from
    // dynupdate. This will avoid a process manager restarting the daemon if we exit. The user may
    // still restart the service if/when they fix the problem.
    pub fn retry_backoff(&self, retry: u8, base_interval: Duration) -> Duration {
        use UpdateError::*;

        match self {
            NoHost | BadAuth | BadAgent | NotDonator | Abuse => Duration::from_secs(FOREVER),
            _ => {
                base_interval
                    + Duration::from_secs(match retry {
                        0 | 1 => 0,
                        2 => 300,
                        3 => 600,
                        4 => 3600,
                        _ => 24 * 60 * 60,
                    })
            }
        }
    }
}

pub fn update(
    username: &str,
    password: &str,
    hostnames: Option<&Vec<String>>,
    ip: IpAddr,
    timeout: Duration,
) -> UpdateResult {
    let url = match hostnames {
        Some(h) => format!(
            "{}?myip={}&hostname={}",
            UPDATE_URL,
            utf8_percent_encode(&ip.to_string(), QUERY_SET),
            utf8_percent_encode(h.join(",").as_str(), QUERY_SET)
        ),
        None => format!(
            "{}?myip={}",
            UPDATE_URL,
            utf8_percent_encode(&ip.to_string(), QUERY_SET)
        ),
    };

    debug!("Updating with url {}", url);

    let r = minreq::get(url)
        .with_header("user-agent", crate::USER_AGENT)
        .with_header(
            "Authorization",
            format!(
                "Basic {}",
                base64_encode(format!("{}:{}", encode_username(username), password))
            ),
        )
        .with_timeout(timeout.as_secs())
        .send()
        .map_err(|e| UpdateError::Connection(format!("{}", e)))?;

    debug!("{:?}", r);

    let body = r
        .as_str()
        .map_err(|e| UpdateError::Unknown(format!("{}", e)))?;

    parse_response(r.status_code, &r.reason_phrase, body)
}

/// Map a No-IP API response to an [`UpdateResult`]. Split out from [`update`]
/// so the (extensive) response-code matrix can be unit-tested without
/// standing up an HTTP mock.
///
/// The body matrix is documented at
/// <https://www.noip.com/integrate/response>.
pub fn parse_response(status: i32, reason: &str, body: &str) -> UpdateResult {
    match status {
        200 => {}
        401 => return Err(UpdateError::BadAuth),
        _ => return Err(UpdateError::StatusCode(status, reason.to_string())),
    }

    match body.trim_end() {
        s if s.starts_with("good ") => Ok(true),
        s if s.starts_with("nochg ") => Ok(false),
        "nohost" => Err(UpdateError::NoHost),
        "badauth" => Err(UpdateError::BadAuth),
        "badagent" => Err(UpdateError::BadAgent),
        "!donator" => Err(UpdateError::NotDonator),
        "abuse" => Err(UpdateError::Abuse),
        "911" => Err(UpdateError::NineOneOne),
        s => Err(UpdateError::Unknown(s.to_owned())),
    }
}

fn encode_username(username: &str) -> String {
    // The No-IP knowledgebase page says to use `:`. Unfortunately that doesn't work with Basic
    // auth. But the dynupdate code is aware of this and handles percent encoded colons and hashes
    // as well.
    //
    // - https://www.noip.com/support/knowledgebase/limit-hostnames-updated-dynamic-dns-client/
    // - https://www.rfc-editor.org/rfc/rfc7617#section-2
    //
    username.replace(':', "%3A")
}

fn base64_encode<T: AsRef<[u8]>>(bytes: T) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn good_response_means_changed() {
        assert!(matches!(
            parse_response(200, "OK", "good 192.0.2.1\n"),
            Ok(true)
        ));
    }

    #[test]
    fn nochg_response_means_unchanged() {
        assert!(matches!(
            parse_response(200, "OK", "nochg 192.0.2.1\n"),
            Ok(false)
        ));
    }

    #[test]
    fn nohost_response_maps_to_nohost_error() {
        assert!(matches!(
            parse_response(200, "OK", "nohost"),
            Err(UpdateError::NoHost)
        ));
    }

    #[test]
    fn badauth_body_maps_to_badauth_error() {
        assert!(matches!(
            parse_response(200, "OK", "badauth"),
            Err(UpdateError::BadAuth)
        ));
    }

    #[test]
    fn badagent_response_maps_to_badagent_error() {
        assert!(matches!(
            parse_response(200, "OK", "badagent"),
            Err(UpdateError::BadAgent)
        ));
    }

    #[test]
    fn not_donator_response_maps_to_not_donator_error() {
        assert!(matches!(
            parse_response(200, "OK", "!donator"),
            Err(UpdateError::NotDonator)
        ));
    }

    #[test]
    fn abuse_response_maps_to_abuse_error() {
        assert!(matches!(
            parse_response(200, "OK", "abuse"),
            Err(UpdateError::Abuse)
        ));
    }

    #[test]
    fn server_outage_911_maps_to_nine_one_one() {
        assert!(matches!(
            parse_response(200, "OK", "911"),
            Err(UpdateError::NineOneOne)
        ));
    }

    #[test]
    fn unknown_body_falls_through() {
        match parse_response(200, "OK", "wat") {
            Err(UpdateError::Unknown(s)) => assert_eq!(s, "wat"),
            other => panic!("expected Unknown, got {other:?}"),
        }
    }

    #[test]
    fn http_401_short_circuits_to_badauth_regardless_of_body() {
        assert!(matches!(
            parse_response(401, "Unauthorized", "ignored"),
            Err(UpdateError::BadAuth)
        ));
    }

    #[test]
    fn non_200_non_401_status_propagates_with_reason() {
        match parse_response(503, "Service Unavailable", "") {
            Err(UpdateError::StatusCode(c, r)) => {
                assert_eq!(c, 503);
                assert_eq!(r, "Service Unavailable");
            }
            other => panic!("expected StatusCode, got {other:?}"),
        }
    }

    #[test]
    fn trailing_whitespace_is_stripped_before_match() {
        assert!(matches!(
            parse_response(200, "OK", "abuse\r\n"),
            Err(UpdateError::Abuse)
        ));
    }

    // Backoff-classification tests guard against accidentally moving a
    // permanent error into the transient bucket (which would cause a process
    // manager to thrash retrying).
    #[test]
    fn permanent_errors_back_off_for_two_weeks() {
        for e in [
            UpdateError::BadAuth,
            UpdateError::BadAgent,
            UpdateError::NoHost,
            UpdateError::NotDonator,
            UpdateError::Abuse,
        ] {
            let d = e.retry_backoff(0, Duration::from_secs(300));
            assert_eq!(
                d,
                Duration::from_secs(FOREVER),
                "{e:?} should back off forever, got {d:?}"
            );
        }
    }

    #[test]
    fn transient_errors_use_base_interval_at_first() {
        let d = UpdateError::Connection("dns".into()).retry_backoff(0, Duration::from_secs(300));
        assert_eq!(d, Duration::from_secs(300));
    }
}
