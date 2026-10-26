export const time = {
  "time.justNow": "just now",
  "time.compact.now": "now",
  "time.compact.minutes": "{{count}}m",
  "time.compact.hours": "{{count}}h",
  "time.compact.days": "{{count}}d",
  "time.yesterdayAt": "Yesterday {{time}}",
} as const satisfies Record<`time.${string}`, string>;
