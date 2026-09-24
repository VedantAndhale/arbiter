use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

macro_rules! id_type {
    ($name:ident, $prefix:literal) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub Uuid);

        impl $name {
            /// Time-ordered (UUIDv7) so ids sort by creation time.
            pub fn new() -> Self {
                Self(Uuid::now_v7())
            }

            pub const PREFIX: &'static str = $prefix;
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}", self.0)
            }
        }

        impl std::str::FromStr for $name {
            type Err = uuid::Error;
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Uuid::parse_str(s.trim_start_matches(concat!($prefix, "_"))).map(Self)
            }
        }
    };
}

id_type!(WorkspaceId, "ws");
id_type!(ProjectId, "prj");
id_type!(TaskId, "tsk");
id_type!(ThreadId, "thr");
id_type!(RunId, "run");

impl WorkspaceId {
    /// The single local workspace used until team features exist.
    pub const LOCAL: WorkspaceId = WorkspaceId(Uuid::nil());
}
