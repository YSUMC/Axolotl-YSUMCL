CREATE TABLE managed_instances (
    server_instance_id TEXT NOT NULL,
    instance_id TEXT NOT NULL,
    revision INTEGER NOT NULL,
    target_revision INTEGER NOT NULL,
    name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    icon_url TEXT NOT NULL DEFAULT '',
    sort INTEGER NOT NULL DEFAULT 0,
    required INTEGER NOT NULL DEFAULT TRUE,
    retired INTEGER NOT NULL DEFAULT FALSE,
    server_address TEXT NOT NULL DEFAULT '',
    server_port INTEGER NOT NULL DEFAULT 0,
    updated INTEGER NOT NULL,

    PRIMARY KEY (server_instance_id)
);

CREATE INDEX managed_instances_instance_id ON managed_instances(instance_id);

CREATE TABLE managed_manifest_cache (
    manifest_url TEXT NOT NULL,
    body TEXT NOT NULL,
    updated INTEGER NOT NULL,

    PRIMARY KEY (manifest_url)
);

-- YSUMC VI starts in the website's light palette. Users can change it afterwards.
UPDATE settings SET theme = 'light';
