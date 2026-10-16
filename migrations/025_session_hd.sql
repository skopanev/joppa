-- The verified Google hd of the sign-in that made each browser session, in lowercase.
-- Sessions made before this column have NULL, which means no hd.
ALTER TABLE joppa.browser_sessions ADD COLUMN IF NOT EXISTS hd text CHECK (hd = lower(hd));
