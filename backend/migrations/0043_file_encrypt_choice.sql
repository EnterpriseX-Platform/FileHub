-- Basket / folder policy: let the person uploading choose to encrypt one file,
-- and which kind of encryption that choice uses.
ALTER TABLE basket_policies ADD COLUMN IF NOT EXISTS file_encrypt_choice BOOLEAN;
ALTER TABLE basket_policies ADD COLUMN IF NOT EXISTS file_encrypt_mode   TEXT;

-- A chunked upload remembers that choice until it finishes.
ALTER TABLE tus_uploads ADD COLUMN IF NOT EXISTS encrypt_file BOOLEAN NOT NULL DEFAULT false;
