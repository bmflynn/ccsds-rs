#![allow(unused)]
use std::{fs, path::Path};

use chrono::{DateTime, FixedOffset, Utc};

use super::error::LeapsecError;

/// Provides number of leapseconds in TAI and UTC
pub trait Leapsecs {
    /// Returns TAI - UTC in seconds at the provided UTC time
    fn leaps_utc(&self, days: u32, picos: u64) -> super::error::Result<i32>;
    /// Returns TAI - UTC in seconds at the provided TAI time
    fn leaps_tai(&self, days: u32, picos: u64) -> super::error::Result<i32>;
}

#[derive(Clone, Debug, Default)]
struct Iers {
    expired: bool,
    expiration: Option<DateTime<Utc>>,
    ignore_expired: bool,
    ignore_out_of_range: bool,
    leaps: Vec<i32>,
    utc: Vec<u64>,
    tai: Vec<u64>,
}

impl Iers {
    const SOURCE_URL: &str = "https://hpiers.obspm.fr/iers/bul/bulc/Leap_Second.dat";

    pub fn from_str(content: &str) -> Result<Self, LeapsecError> {
        let mut prev = None;
        let mut iers = Iers::default();
        for (lineno, line) in content
            .lines()
            .filter(|s| s.starts_with(" "))
            .enumerate()
            .map(|(i, z)| (i + 1, z))
        {
            if line.starts_with("#") {
                if let Some(expr) = Self::parse_expiration(line) {
                    iers.expired = expr < Utc::now();
                }
                continue;
            }

            match Self::parse_record(line) {
                Ok((timestamp, leaps)) => {
                    if let Some(prev) = prev {
                        if leaps - prev != 1 {
                            return Err(LeapsecError::Parse(format!(
                                "records more that 1s apart at line={lineno}"
                            )));
                        }
                    }
                    prev = Some(leaps);
                    iers.leaps.push(leaps);
                    iers.utc.push(timestamp);
                    let tai = ((i64::try_from(timestamp).unwrap()) + leaps as i64)
                        .try_into()
                        .unwrap();
                    iers.tai.push(tai);
                }
                Err(err) => {
                    return Err(LeapsecError::Parse(format!(
                        "invalid record at line={lineno}: {err}"
                    )))
                }
            }
        }

        // Extract file expiration if possible
        for line in content.lines().filter(|s| s.starts_with("#")) {
            if let Some(expr) = Self::parse_expiration(line) {
                iers.expired = expr < Utc::now();
                iers.expiration = Some(expr);
                break;
            }
        }

        Ok(iers)
    }

    pub fn from_file<P: AsRef<Path>>(
        path: P,
    ) -> std::result::Result<Self, Box<dyn std::error::Error>> {
        let dat = fs::read(&path)?;
        let content = std::str::from_utf8(&dat)?;
        Ok(Self::from_str(content)?)
    }

    fn parse_expiration(line: &str) -> Option<DateTime<Utc>> {
        let parts: Vec<&str> = line.split("File expires on ").collect();
        if parts.len() != 2 {
            return None;
        }
        let timestr = format!("{} 00:00:00 +0000", parts[1]);
        match DateTime::parse_from_str(&timestr, "%d %B %Y %H:%M:%S %z") {
            Ok(dt) => Some(dt.into()),
            Err(_) => None,
        }
    }

    /// Parse out UTC timestamp from date and TAI-UTC seconds.
    ///
    /// # Panics
    /// If parsed timestamp is before Jan 1, 1970
    fn parse_record(line: &str) -> std::result::Result<(u64, i32), String> {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() != 5 {
            return Err("not enough components".to_string());
        }

        let timestr = &format!("{}-{:0>2}-{:0>2}T00:00:00Z", parts[3], parts[2], parts[1]);
        let dt: DateTime<FixedOffset> = match DateTime::parse_from_rfc3339(timestr) {
            Ok(dt) => dt,
            Err(e) => return Err("failed to parse date string".to_string()),
        };

        Ok((
            // should't ever have time < epoch (negative)
            dt.timestamp().try_into().unwrap(),
            parts[4]
                .parse::<i32>()
                .map_err(|_| format!("failed to parse leap secs {}", parts[4]))?,
        ))
    }

    fn find_leaps(times: &[u64], time: u64) -> Result<usize, LeapsecError> {
        for (i, leap_time) in times.iter().enumerate().rev() {
            if time >= *leap_time {
                return Ok(i);
            }
        }
        Err(LeapsecError::OutOfRange)
    }
}

impl Leapsecs for Iers {
    fn leaps_utc(&self, days: u32, picos: u64) -> super::error::Result<i32> {
        let time = days as u64 * 86_400 + picos.saturating_div(1_000_000_000_000);
        let idx = Self::find_leaps(&self.utc, time)?;
        Ok(self.leaps[idx])
    }

    fn leaps_tai(&self, days: u32, picos: u64) -> super::error::Result<i32> {
        let time = days as u64 * 86_400 + picos.saturating_div(1_000_000_000_000);
        let idx = Self::find_leaps(&self.tai, time)?;
        Ok(self.leaps[idx])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_record() {
        let line = "41317.0    1  1 1972       10\n";

        assert_eq!(Ok((63_072_000, 10)), Iers::parse_record(line));
    }

    #[test]
    fn test_from_str() {
        let dat = "
#  
#
#  File expires on 28 June 2025
#
#
#    MJD        Date        TAI-UTC (s)
#           day month year
#    ---    --------------   ------   
#
    41317.0    1  1 1972       10
    41499.0    1  7 1972       11
    41683.0    1  1 1973       12";
        let iers = Iers::from_str(dat).unwrap();

        assert_eq!(iers.leaps, vec![10, 11, 12]);

        let expected: DateTime<Utc> = DateTime::parse_from_rfc3339("2025-06-28T00:00:00Z")
            .unwrap()
            .into();
        assert_eq!(Some(expected), iers.expiration);
    }

    #[test]
    fn test_leaps_utc() {
        let mut iers = Iers::default();
        iers.leaps = vec![1, 10];
        iers.utc = vec![63_072_000, 63_072_000];
        iers.tai = vec![63_072_000, 63_072_000];
    }
}
