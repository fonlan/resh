import React from "react"
import twemoji from "twemoji"

interface EmojiTextProps {
  text: string
  className?: string
}

/**
 * A component that renders text with emojis replaced by Twemoji SVG images.
 * This ensures consistent emoji rendering across all platforms (especially Windows)
 * and avoids font-related character spacing issues.
 */
export const EmojiText: React.FC<EmojiTextProps> = ({
  text,
  className = "",
}) => {
  // Parse the text into Twemoji images
  // We use dangerouslySetInnerHTML because twemoji.parse returns an HTML string
  // with <img> tags for emojis.
  const parsedHtml = twemoji.parse(text, {
    folder: "svg",
    ext: ".svg",
    base: "https://cdn.jsdelivr.net/gh/twitter/twemoji@14.0.2/assets/",
  })

  return (
    <span
      className={`emoji-container ${className}`}
      dangerouslySetInnerHTML={{ __html: parsedHtml }}
    />
  )
}
