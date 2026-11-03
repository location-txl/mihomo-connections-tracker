CREATE TABLE connections (
    id TEXT PRIMARY KEY NOT NULL,
    start INTEGER NOT NULL,
    first_seen INTEGER NOT NULL,
    last_seen INTEGER NOT NULL,
    disappeared_at INTEGER,
    status TEXT NOT NULL CHECK(status IN ('active', 'unknown', 'disappeared')),
    last_upload INTEGER NOT NULL,
    last_download INTEGER NOT NULL,
    observed_upload INTEGER NOT NULL DEFAULT 0,
    observed_download INTEGER NOT NULL DEFAULT 0,
    airport TEXT,
    route_kind TEXT NOT NULL,
    node TEXT,
    attribution_reason TEXT,
    domain TEXT,
    destination_ip TEXT NOT NULL,
    network TEXT NOT NULL,
    process TEXT NOT NULL,
    rule TEXT NOT NULL,
    rule_payload TEXT NOT NULL,
    metadata TEXT NOT NULL,
    chains TEXT NOT NULL,
    provider_chains TEXT NOT NULL
);
CREATE INDEX connections_airport ON connections(airport);
CREATE INDEX connections_domain ON connections(domain);
CREATE INDEX connections_status ON connections(status);

CREATE TABLE connection_groups (
    connection_id TEXT NOT NULL REFERENCES connections(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    PRIMARY KEY(connection_id, name)
);
CREATE INDEX connection_groups_name ON connection_groups(name, connection_id);

CREATE TABLE connection_daily_usage (
    connection_id TEXT NOT NULL REFERENCES connections(id) ON DELETE CASCADE,
    day TEXT NOT NULL,
    upload INTEGER NOT NULL,
    download INTEGER NOT NULL,
    PRIMARY KEY(connection_id, day)
);
CREATE INDEX connection_daily_day ON connection_daily_usage(day, connection_id);

CREATE TABLE daily_totals (
    day TEXT NOT NULL,
    dimension TEXT NOT NULL CHECK(dimension IN ('global', 'airport', 'group')),
    name TEXT NOT NULL,
    route_kind TEXT NOT NULL,
    upload INTEGER NOT NULL,
    download INTEGER NOT NULL,
    PRIMARY KEY(day, dimension, name, route_kind)
);
CREATE INDEX daily_totals_dimension ON daily_totals(dimension, day, name);

-- 不关联明细外键，避免 12 个月清理连带删除诊断证据。
CREATE TABLE attribution_anomalies (
    id INTEGER PRIMARY KEY,
    connection_id TEXT NOT NULL,
    reason TEXT NOT NULL,
    first_seen INTEGER NOT NULL,
    last_seen INTEGER NOT NULL,
    occurrences INTEGER NOT NULL,
    first_raw_connection TEXT NOT NULL,
    latest_raw_connection TEXT NOT NULL,
    raw_context TEXT NOT NULL,
    UNIQUE(connection_id, reason)
);

CREATE TABLE collection_gaps (
    id INTEGER PRIMARY KEY,
    started_at INTEGER NOT NULL,
    ended_at INTEGER,
    reason TEXT NOT NULL
);
CREATE UNIQUE INDEX one_open_gap ON collection_gaps((1)) WHERE ended_at IS NULL;

CREATE TABLE collector_state (
    id INTEGER PRIMARY KEY CHECK(id = 1),
    connected INTEGER NOT NULL DEFAULT 0,
    last_success_at INTEGER,
    last_error TEXT
);
INSERT INTO collector_state(id) VALUES(1);
