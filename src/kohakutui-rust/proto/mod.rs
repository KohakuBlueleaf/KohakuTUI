//! Wire protocol: Ops (Python to engine), Events (engine to Python), the dynamic value type and
//! the framing used by subprocess and network transports (design §4).

/// Protocol version as (major, minor). Peers must agree on major; minor only adds.
pub const PROTOCOL: (u16, u16) = (1, 0);
