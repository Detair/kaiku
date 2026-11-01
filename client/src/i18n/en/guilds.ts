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
} as const satisfies Record<`guilds.${string}`, string>;
