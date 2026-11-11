export const guilds = {
  "guilds.create.namePlaceholder": "e.g. Friday Game Night",
  "guilds.create.nameRequired": "Enter a server name to continue.",
  "guilds.create.descriptionPlaceholder": "e.g. Where we plan our weekly sessions",
  "guilds.create.discoverableLabel": "Make server discoverable",
  "guilds.create.tagPlaceholder": "e.g. co-op",
  "guilds.create.tagRequired": "Type a tag first.",
  "guilds.create.bannerChoose": "Choose image",
  "guilds.create.bannerUrlPlaceholder": "e.g. https://example.com/banner.png",
  "guilds.create.discoveryOff":
    "The server browser is turned off on this Kaiku instance, so new servers are joined by invite only.",
  "guilds.settings.discoveryOff":
    "The server browser is turned off on this Kaiku instance, so servers cannot be listed there. An instance admin can turn it on.",
  "guilds.join.title": "Join a Server",
  "guilds.join.label": "Invite link or code",
  "guilds.join.placeholder": "e.g. https://example.com/invite/aBcD1234",
  "guilds.join.hint": "Paste the invite link someone sent you. The short code at the end also works.",
  "guilds.join.invalid": "This does not look like an invite link. Paste the whole link you were sent.",
  "guilds.join.failed": "Could not join the server. The invite may have expired. Ask for a new link.",
  "guilds.join.joining": "Joining...",
  "guilds.join.submit": "Join Server",
} as const satisfies Record<`guilds.${string}`, string>;
