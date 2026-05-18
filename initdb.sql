


CREATE TABLE IF NOT EXISTS keys (
    id SERIAL NOT NULL PRIMARY KEY,
    rsa_key BYTEA NOT NULL,
    created_at TIMESTAMPTZ DEFAULT NOW()
);


CREATE TABLE IF NOT EXISTS forms (
    id BIGSERIAL NOT NULL PRIMARY KEY,
    name VARCHAR(255) NOT NULL,
    description TEXT,
    created_at TIMESTAMPTZ DEFAULT NOW(),
    owner VARCHAR NOT NULL,
    fields BYTEA NOT NULL,
    mentioned_emails TEXT[] DEFAULT '{}',
    deadline TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_forms_owner ON forms(owner);

CREATE TABLE IF NOT EXISTS form_allowed_participants (
    form_id BIGINT NOT NULL REFERENCES forms(id) ON DELETE CASCADE,
    participant VARCHAR NOT NULL,
    accepted BOOLEAN DEFAULT FALSE,

    PRIMARY KEY (form_id, participant)
);


CREATE TABLE IF NOT EXISTS form_submissions (
    form_id BIGINT NOT NULL REFERENCES forms(id) ON DELETE CASCADE,
    data BYTEA NOT NULL,
    created_at TIMESTAMPTZ DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS otp_codes (
    id BIGSERIAL NOT NULL PRIMARY KEY,
    email VARCHAR NOT NULL,
    code VARCHAR NOT NULL,
    form_id BIGINT REFERENCES forms(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ DEFAULT NOW(),
    expires_at TIMESTAMPTZ NOT NULL,
    used BOOLEAN DEFAULT FALSE
);

-- Ensure form_id is nullable if the table already exists
ALTER TABLE otp_codes ALTER COLUMN form_id DROP NOT NULL;

CREATE INDEX IF NOT EXISTS idx_otp_codes_form_email ON otp_codes(form_id, email);
CREATE INDEX IF NOT EXISTS idx_otp_codes_expires ON otp_codes(expires_at) WHERE used = FALSE;
 
 
 CREATE TABLE IF NOT EXISTS secrets (
     id SERIAL PRIMARY KEY,
     key_data BYTEA NOT NULL,
     created_at TIMESTAMPTZ DEFAULT NOW(),
     expires_at TIMESTAMPTZ
 );

CREATE TABLE IF NOT EXISTS user_subscriptions (
    owner VARCHAR NOT NULL PRIMARY KEY,
    tier VARCHAR NOT NULL DEFAULT 'free' CHECK (tier IN ('free', 'pro', 'team')),
    razorpay_subscription_id VARCHAR UNIQUE,
    razorpay_plan_id VARCHAR,
    -- subscription_status mappings:
    -- 0 = inactive
    -- 1 = pending
    -- 2 = active
    -- 3 = cancelled
    -- 4 = halted
    -- 5 = completed
    subscription_status SMALLINT NOT NULL DEFAULT 0,
    current_period_end TIMESTAMPTZ,
    created_at TIMESTAMPTZ DEFAULT NOW(),
    updated_at TIMESTAMPTZ DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_user_subscriptions_owner ON user_subscriptions(owner);
CREATE INDEX IF NOT EXISTS idx_user_subscriptions_rzpay ON user_subscriptions(razorpay_subscription_id) WHERE razorpay_subscription_id IS NOT NULL;

-- Migration: add Razorpay columns if table already exists (safe to run multiple times)
ALTER TABLE user_subscriptions ADD COLUMN IF NOT EXISTS razorpay_subscription_id VARCHAR UNIQUE;
ALTER TABLE user_subscriptions ADD COLUMN IF NOT EXISTS razorpay_plan_id VARCHAR;
ALTER TABLE user_subscriptions ADD COLUMN IF NOT EXISTS subscription_status SMALLINT NOT NULL DEFAULT 0;
ALTER TABLE user_subscriptions ADD COLUMN IF NOT EXISTS current_period_end TIMESTAMPTZ;
