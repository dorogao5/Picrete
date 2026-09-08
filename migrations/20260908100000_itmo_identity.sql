-- ITMO subjects are linked explicitly; email/ISU never silently merge accounts.
CREATE TABLE external_identities (
    issuer TEXT NOT NULL,
    subject TEXT NOT NULL,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    profile JSONB NOT NULL DEFAULT '{}',
    synced_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (issuer, subject),
    UNIQUE (issuer, user_id)
);
CREATE TABLE oidc_login_states (
    state_hash TEXT PRIMARY KEY,
    binding_hash TEXT NOT NULL,
    verifier TEXT NOT NULL,
    nonce TEXT NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    consent BOOLEAN NOT NULL,
    link_user_id TEXT REFERENCES users(id) ON DELETE CASCADE
);
CREATE INDEX oidc_login_states_expiry ON oidc_login_states(expires_at);
