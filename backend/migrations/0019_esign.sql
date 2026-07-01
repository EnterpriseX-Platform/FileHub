-- 0019_esign.sql — electronic signatures (TOR Annex A: ระบบลายมือชื่ออิเล็กทรอนิกส์,
-- per พ.ร.บ. ธุรกรรมทางอิเล็กทรอนิกส์ พ.ศ. 2544 §9). Models a reusable signature
-- library plus ordered/parallel multi-signer sign requests with integrity
-- hashing (SHA-256 of the document at request and at each signing).

-- A user's reusable signature marks (drawn / typed / uploaded), stored as a
-- data-URL so the viewer can overlay it at the placement coordinates.
CREATE TABLE signatures (
    id         UUID PRIMARY KEY,
    user_id    TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    label      TEXT NOT NULL DEFAULT 'Signature',
    kind       TEXT NOT NULL DEFAULT 'drawn',    -- drawn | typed | uploaded
    image      TEXT NOT NULL,                     -- data URL (png/svg) of the mark
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX signatures_user_idx ON signatures (user_id, created_at DESC);

-- One signing request over one file, with one or more signers.
CREATE TABLE signature_requests (
    id           UUID PRIMARY KEY,
    file_id      UUID NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    created_by   TEXT NOT NULL REFERENCES users(id),
    order_mode   TEXT NOT NULL DEFAULT 'sequential',  -- sequential | parallel
    status       TEXT NOT NULL DEFAULT 'pending',      -- pending | completed | declined | cancelled | expired
    message      TEXT,
    doc_hash     TEXT,                                 -- SHA-256 of the file bytes when the request was created
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at   TIMESTAMPTZ,
    completed_at TIMESTAMPTZ
);
CREATE INDEX signature_requests_file_idx ON signature_requests (file_id);
CREATE INDEX signature_requests_pending_idx ON signature_requests (status) WHERE status = 'pending';

-- Per-signer state: order, decision, visible placement, and the integrity hash
-- captured at the moment they signed.
CREATE TABLE signature_signers (
    id           UUID PRIMARY KEY,
    request_id   UUID NOT NULL REFERENCES signature_requests(id) ON DELETE CASCADE,
    user_id      TEXT NOT NULL REFERENCES users(id),
    seq          INT  NOT NULL DEFAULT 0,              -- signing order (used when sequential)
    status       TEXT NOT NULL DEFAULT 'pending',      -- pending | signed | declined
    page         INT,                                  -- 1-based page for the visible mark
    pos_x        REAL, pos_y REAL, width REAL, height REAL,
    signature_id UUID REFERENCES signatures(id) ON DELETE SET NULL,
    signed_hash  TEXT,                                 -- SHA-256 of the doc bytes at signing
    signed_at    TIMESTAMPTZ,
    ip           TEXT
);
CREATE INDEX signature_signers_req_idx  ON signature_signers (request_id, seq);
CREATE INDEX signature_signers_user_idx ON signature_signers (user_id, status);
