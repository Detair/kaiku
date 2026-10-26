export const time = {
  "time.justNow": "just now",
  "time.yesterdayAt": "Yesterday {{time}}",
} as const satisfies Record<`time.${string}`, string>;
