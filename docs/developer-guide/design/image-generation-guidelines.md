# Kaiku Image Generation Guidelines

This document serves as the official style guide for generating brand assets, specifically featuring the mascot "Floki", for the Kaiku project. To maintain a consistent, premium, and recognizable aesthetic, all generated illustrations must adhere strictly to these prompt rules.

## The Mascot: Floki
- **Subject**: A Suomenlapinkoira (Finnish Lapphund).
- **Appearance**: Fluffy, wolf-like but friendly appearance. Usually depicted with a thick double coat, perky ears, and an alert, intelligent expression.

## The Aesthetic: CachyOS Nordic & Modern Gaming
Kaiku utilizes the CachyOS Nordic theme palette. The visual style should feel like a premium, privacy-focused gaming communication platform.
- **Colors**: Deep dark charcoal/blue slate backgrounds.
- **Accents**: Vibrant, luminous "frosty cyan" (`#8FBCBB`, `#81A1C1`) and "aurora purple" (`#B48EAD`).
- **Style**: Vector art, flat but with depth, sleek, stylized, glassmorphism elements, slight neon glow. Avoid hyper-realism or overly cartoonish "Disney" styles.

## The Prompt Structure
When generating new feature illustrations using image generation AI (like Midjourney, DALL-E 3, or internal models), use the following base formula, and append the specific feature action at the end:

### The Golden Base Prompt
> `A sleek, modern stylized vector illustration of a fluffy Suomenlapinkoira (Finnish Lapphund) dog. The art style is premium gaming aesthetic, using a CachyOS Nordic color palette: dark charcoal and slate backgrounds, accented with vibrant frosty cyan and glowing aurora purple. Clean lines, subtle neon glow, glassmorphism elements, flat design with depth. The dog is [INSERT SPECIFIC ACTION/SETTING HERE]. Highly detailed, minimalist background, transparent background feel.`

### Example Modifier: Admin Dashboard
> `... The dog is wearing futuristic hacker glasses and typing on a glowing holographic keyboard, multiple data graphs floating in the air around him.`

## Best Practices
1. **Consistency**: Always ensure the dog strongly resembles a Finnish Lapphund (fluffy, not a Golden Retriever, not a tiny Pomeranian).
2. **Color Control**: Force the AI to use cyan and purple for highlights. If it generates warm colors (oranges, reds), reject the image.
3. **Backgrounds**: Request "isolated on a dark slate background" or "transparent background feel" to make it easier to extract the element for web use via CSS `mix-blend-mode: screen` or background removal tools.
4. **Tail Artifacts**: Be careful of AI artifacts like two tails or strange anatomical anomalies. Always review the dog's structure.
