CREATE TABLE managed_installations (
    instance_id TEXT PRIMARY KEY REFERENCES instances(id) ON DELETE CASCADE,
    spec_json TEXT NOT NULL
);
