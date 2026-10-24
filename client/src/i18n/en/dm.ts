export const dm = {
  "dm.header.selectConversation": "Select a conversation",
  "dm.header.unknownUser": "Unknown",
  "dm.header.changeIcon": "Change icon (Max {{limit}})",
  "dm.header.iconAlt": "DM Icon",
  "dm.header.uploadIconFailed": "Failed to upload icon",
  "dm.header.clickToRename": "Click to rename",
  "dm.header.renameFailed": "Could not rename conversation. Please try again.",
  "dm.header.members": "{{count}} members",
  "dm.header.startCall": "Start voice call",
  "dm.header.callInProgress": "Call in progress",
} as const satisfies Record<`dm.${string}`, string>;
