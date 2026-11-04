-- F5c (L1b): an `admin` enrolment sets up (`setup`) or recovers
-- (`recovery`) an account; at `done` an existing account gets an `admin`
-- request of the matching purpose.
ALTER TABLE enrollments
    ADD COLUMN admin_mode TEXT CHECK (admin_mode IN ('setup', 'recovery'));
