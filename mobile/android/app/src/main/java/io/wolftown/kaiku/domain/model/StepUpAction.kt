package io.wolftown.kaiku.domain.model

/** The step-up action table of the C26–C29 common rules (F7), copied once per platform. Append-only. */
enum class StepUpAction(val wire: String) {
    PASSKEY_ADD("passkey.add"),
    PASSKEY_REMOVE("passkey.remove"),
    KEY_DEVICE_REMOVE("key_device.remove"),
    ENROLLMENT_ISSUE("enrollment.issue"),
    PASSWORD_SET("password.set"),
    PASSWORD_REMOVE("password.remove"),
    TOTP_SET("totp.set"),
    TOTP_REMOVE("totp.remove"),
    RECOVERY_CODES_CREATE("recovery_codes.create"),
    IDENTITY_LINK("identity.link"),
    IDENTITY_UNLINK("identity.unlink"),
    SESSIONS_END("sessions.end"),
    RECOVERY_CONTACT_SET("recovery_contact.set"),
    REQUEST_CANCEL("request.cancel"),
    EVENT_REPUDIATE("event.repudiate"),
    ACCOUNT_DELETE("account.delete"),
    ADMIN_ELEVATE("admin.elevate"),
    ADMIN_ENROLLMENT_ISSUE("admin.enrollment.issue"),
    ADMIN_AUTHENTICATOR_REMOVE("admin.authenticator.remove"),
    ADMIN_MAY_APPROVE("admin.may_approve"),
    ADMIN_FACTOR_SET_STATE("admin.factor.set_state"),
    ADMIN_RECOVERY_CONTACT_SET("admin.recovery_contact.set");

    companion object {
        fun fromWire(wire: String): StepUpAction? = entries.firstOrNull { it.wire == wire }
    }
}
