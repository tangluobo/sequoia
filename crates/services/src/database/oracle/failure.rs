//! Preserve typed transport failures for the shared bounded read-only recovery.
use sqlx::Error;
use std::{error::Error as _, io};

fn kind(value: &oracledb::Error) -> Option<io::ErrorKind> {
    use oracledb::ErrorKind::*;
    match value.kind() {
        DeadConnection
        | NotConnected
        | UnableToRecover
        | UnknownTtcMessageType(..)
        | UnknownServerSidePiggyback(_) => Some(io::ErrorKind::ConnectionAborted),
        CallTimeoutExceeded => Some(io::ErrorKind::TimedOut),
        StreamOperation => value
            .source()
            .and_then(|cause| cause.downcast_ref::<io::Error>())
            .map(io::Error::kind),
        DbError(error)
            if matches!(
                error.code(),
                28 | 1012
                    | 1033
                    | 1034
                    | 1089
                    | 1090
                    | 2396
                    | 3113
                    | 3114
                    | 3135
                    | 12537
                    | 12541
                    | 12547
                    | 12570
                    | 12571
            ) =>
        {
            Some(io::ErrorKind::ConnectionAborted)
        }
        _ => None,
    }
}
pub(super) fn driver_error(value: oracledb::Error) -> Error {
    match kind(&value) {
        Some(kind) => Error::Io(io::Error::new(kind, value)),
        None => Error::Protocol(value.to_string()),
    }
}
pub(super) fn lob_error(value: io::Error) -> Error {
    if let Some(driver) = value
        .get_ref()
        .and_then(|cause| cause.downcast_ref::<oracledb::Error>())
        && let Some(kind) = kind(driver)
    {
        return Error::Io(io::Error::new(kind, value));
    }
    Error::Io(value)
}
