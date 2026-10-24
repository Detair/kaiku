export const qr = {
  "qr.title": "Link Mobile Device",
  "qr.instructions": "Scan this code with the Kaiku mobile app to sign in",
  "qr.alt": "QR login code",
  "qr.expiresIn": "Expires in {{seconds}}s",
  "qr.expired": "Code expired",
  "qr.generateNew": "Generate new code",
  "qr.tryAgain": "Try again",
  "qr.noServer": "Can't create a login code because the server address is unknown. Sign out and sign in again.",
  "qr.failedTitle": "QR Code Failed",
  "qr.failedMessage": "Could not generate QR login code.",
} as const satisfies Record<`qr.${string}`, string>;
