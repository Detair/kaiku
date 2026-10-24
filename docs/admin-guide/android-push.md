# Android push notifications (FCM)

The Android app shows new messages while it is closed by receiving a short,
content-free wake signal (message type, channel id, count) and then fetching
the messages from your server itself. No message text goes through Google.

On phones with Google Play services the app uses Firebase Cloud Messaging
(FCM). It only does so when the APK and your server belong to the **same
Firebase project**; otherwise FCM stays off and the app falls back to other
transports (UnifiedPush, where available) or to fetching on open.

Two configuration items are involved, one per side:

| Where | Item | What it is |
|-------|------|------------|
| Release workflow (GitHub Actions secret, Environment `release`) | `ANDROID_GOOGLE_SERVICES_JSON_BASE64` | The app's `google-services.json`, base64-encoded. Baked into the APK at build time |
| Server environment | `FCM_SERVICE_ACCOUNT_FILE` or `FCM_SERVICE_ACCOUNT_JSON` | A service-account key that may send FCM messages for the project (see [environment variables](configuration/environment.md#push-notifications-fcm)) |

## 1. Create the Firebase project

1. In the [Firebase console](https://console.firebase.google.com/), create a
   project (Google Analytics is not needed).
2. Add an **Android app** with package name `io.wolftown.kaiku`.
3. Download `google-services.json`. It identifies the project (`project_id`)
   and the app; it is not a credential, but keep it out of the repository
   (`mobile/android/app/google-services.json` is git-ignored).

## 2. Give the release build the app config

```bash
base64 -w0 google-services.json
```

Store the output as the secret `ANDROID_GOOGLE_SERVICES_JSON_BASE64` in the
repository's `release` Environment. The `build-android` job writes it to
`mobile/android/app/google-services.json` before building and deletes it
afterwards.

- **Publish runs** fail if the secret is missing.
- **Dry runs** build without it: the APK then has FCM compiled in but inactive.

Local builds behave the same way: without `app/google-services.json` the build
succeeds and FCM is off; place the file there to build with FCM.

## 3. Give the server a service account

1. In the Google Cloud console for the same project, open
   **IAM & Admin → Service Accounts** and create a service account.
2. Grant it the role **Firebase Cloud Messaging API Admin**
   (`roles/firebasecloudmessaging.admin`), and make sure the
   **Firebase Cloud Messaging API (V1)** is enabled for the project.
3. Create a JSON key for it and provide it to the server, preferably as a file
   (`FCM_SERVICE_ACCOUNT_FILE`, e.g. a mounted Kubernetes secret). The key is a
   credential: store it like any other secret.

The server reports its project at `GET /api/me/push-subscriptions/config`;
the app compares it with the project baked into the APK.

## Checking it works

- With FCM configured, the config endpoint lists `fcm` among `providers` with
  the expected `project_id`.
- After signing in on a phone with Google Play services and allowing
  notifications, the phone appears in the user's push subscriptions with
  provider `fcm`.
- Signing out removes that device's subscription.

If the projects differ (for example, a server pointed at a different Firebase
project than the one the APK was built with), the app does not use FCM. Fix
the server key or rebuild the APK with the matching `google-services.json`.
