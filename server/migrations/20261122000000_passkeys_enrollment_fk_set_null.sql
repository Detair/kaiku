-- `passkeys.enrollment_id` (20261120000700) referenced `enrollments`
-- without a delete action, so the 30-day cleanup of ended enrolments
-- (`cleanup_finished_approvals`) failed once a completed browser setup-link
-- claim aged out, and rolled back the approval cleanup of the same run.
-- The passkey outlives its enrolment; the link is cleared instead, as
-- `approval_devices.enrollment_id` (no foreign key) simply dangles.
ALTER TABLE passkeys
    DROP CONSTRAINT passkeys_enrollment_id_fkey,
    ADD CONSTRAINT passkeys_enrollment_id_fkey
        FOREIGN KEY (enrollment_id) REFERENCES enrollments(id) ON DELETE SET NULL;
