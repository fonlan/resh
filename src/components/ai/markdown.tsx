import React, { useState } from "react"
import type { Components } from "react-markdown"
import remarkGfm from "remark-gfm"
import { Check, Copy, Terminal } from "lucide-react"
import { useTranslation } from "../../i18n"

export const MARKDOWN_REMARK_PLUGINS = [remarkGfm]

const CodeBlock = ({
  children,
  className,
}: {
  children: React.ReactNode
  className?: string
}) => {
  const { t } = useTranslation()
  const [copied, setCopied] = useState(false)

  const codeContent = (() => {
    if (typeof children === "string") return children
    if (Array.isArray(children)) return children.join("")
    return String(children)
  })().replace(/\n$/, "")

  const language = className ? className.replace("language-", "") : "text"

  const handleCopy = async () => {
    try {
      await navigator.clipboard.writeText(codeContent)
      setCopied(true)
      setTimeout(() => setCopied(false), 2000)
    } catch (err) {
      // Failed to copy
    }
  }

  const handleInsert = () => {
    window.dispatchEvent(
      new CustomEvent("paste-snippet", { detail: codeContent }),
    )
  }

  return (
    <div className="my-2 rounded-md overflow-hidden bg-black/30 border border-[var(--glass-border)]">
      <div className="flex justify-between items-center px-3 py-1.5 bg-white/5 border-b border-[var(--glass-border)]">
        <span className="text-[11px] text-[var(--text-muted)] uppercase font-mono">
          {language}
        </span>
        <div className="flex gap-1">
          <button
            type="button"
            className="bg-transparent border-0 text-[var(--text-muted)] cursor-pointer p-1 rounded flex items-center justify-center transition-all duration-200 hover:bg-[var(--bg-tertiary)] hover:text-[var(--text-primary)]"
            onClick={handleCopy}
            title={t.ai.tool.copyCode}
          >
            {copied ? (
              <Check size={14} className="text-green-500" />
            ) : (
              <Copy size={14} />
            )}
          </button>
          <button
            type="button"
            className="bg-transparent border-0 text-[var(--text-muted)] cursor-pointer p-1 rounded flex items-center justify-center transition-all duration-200 hover:bg-[var(--bg-tertiary)] hover:text-[var(--text-primary)]"
            onClick={handleInsert}
            title={t.ai.tool.insertToTerminal}
          >
            <Terminal size={14} />
          </button>
        </div>
      </div>
      <pre className="m-0 p-3 overflow-x-auto font-mono text-[12px]">
        <code className={className}>{children}</code>
      </pre>
    </div>
  )
}

export const MARKDOWN_COMPONENTS: Components = {
  p({ className, children, ...props }) {
    return (
      <p
        {...props}
        className={`my-2 leading-6 whitespace-pre-wrap ${className ?? ""}`}
      >
        {children}
      </p>
    )
  },
  h1({ className, children, ...props }) {
    return (
      <h1
        {...props}
        className={`mt-3 mb-2 text-[18px] leading-[1.3] font-semibold ${className ?? ""}`}
      >
        {children}
      </h1>
    )
  },
  h2({ className, children, ...props }) {
    return (
      <h2
        {...props}
        className={`mt-3 mb-2 text-[16px] leading-[1.35] font-semibold ${className ?? ""}`}
      >
        {children}
      </h2>
    )
  },
  h3({ className, children, ...props }) {
    return (
      <h3
        {...props}
        className={`mt-2.5 mb-1.5 text-[14px] leading-[1.4] font-semibold ${className ?? ""}`}
      >
        {children}
      </h3>
    )
  },
  ul({ className, children, ...props }) {
    return (
      <ul
        {...props}
        className={`my-2 ml-5 list-disc space-y-1 ${className ?? ""}`}
      >
        {children}
      </ul>
    )
  },
  ol({ className, children, ...props }) {
    return (
      <ol
        {...props}
        className={`my-2 ml-5 list-decimal space-y-1 ${className ?? ""}`}
      >
        {children}
      </ol>
    )
  },
  li({ className, children, ...props }) {
    return (
      <li {...props} className={`leading-6 ${className ?? ""}`}>
        {children}
      </li>
    )
  },
  blockquote({ className, children, ...props }) {
    return (
      <blockquote
        {...props}
        className={`my-2 border-l-[3px] border-[var(--accent-primary)] bg-black/10 px-3 py-2 italic text-[var(--text-secondary)] ${className ?? ""}`}
      >
        {children}
      </blockquote>
    )
  },
  a({ className, children, ...props }) {
    return (
      <a
        {...props}
        className={`underline underline-offset-2 text-[var(--accent-primary)] hover:opacity-85 transition-opacity ${className ?? ""}`}
      >
        {children}
      </a>
    )
  },
  hr({ className, ...props }) {
    return (
      <hr
        {...props}
        className={`my-3 border-0 border-t border-[var(--glass-border)] ${className ?? ""}`}
      />
    )
  },
  strong({ className, children, ...props }) {
    return (
      <strong {...props} className={`font-semibold ${className ?? ""}`}>
        {children}
      </strong>
    )
  },
  em({ className, children, ...props }) {
    return (
      <em {...props} className={`italic ${className ?? ""}`}>
        {children}
      </em>
    )
  },
  pre({ children }: { children?: React.ReactNode }) {
    return <>{children}</>
  },
  code({ className, children, ...props }) {
    const markdownCodeProps =
      props as React.ComponentPropsWithoutRef<"code"> & { inline?: boolean }
    const { inline, ...codeProps } = markdownCodeProps
    const codeContent = (() => {
      if (typeof children === "string") return children
      if (Array.isArray(children)) return children.join("")
      return String(children ?? "")
    })()
    const hasLanguageClass = !!className?.includes("language-")
    const shouldRenderBlock =
      inline === false || hasLanguageClass || codeContent.includes("\n")

    if (shouldRenderBlock) {
      return <CodeBlock className={className}>{children}</CodeBlock>
    }

    return (
      <code
        {...codeProps}
        className={`bg-transparent text-[var(--text-primary)] font-inherit text-inherit p-0 px-1 italic opacity-90 rounded border border-[var(--glass-border)] ${className ?? ""}`}
      >
        {children}
      </code>
    )
  },
  table({
    className,
    children,
    ...props
  }: React.ComponentPropsWithoutRef<"table">) {
    return (
      <div className="my-2 w-full overflow-x-auto">
        <table
          {...props}
          className={`w-full min-w-max border-collapse border border-[var(--glass-border)] text-[12.5px] ${className ?? ""}`}
        >
          {children}
        </table>
      </div>
    )
  },
  thead({
    className,
    children,
    ...props
  }: React.ComponentPropsWithoutRef<"thead">) {
    return (
      <thead {...props} className={`bg-black/20 ${className ?? ""}`}>
        {children}
      </thead>
    )
  },
  tr({ className, children, ...props }: React.ComponentPropsWithoutRef<"tr">) {
    return (
      <tr {...props} className={`${className ?? ""}`}>
        {children}
      </tr>
    )
  },
  th({ className, children, ...props }: React.ComponentPropsWithoutRef<"th">) {
    return (
      <th
        {...props}
        className={`border border-[var(--glass-border)] px-2.5 py-1.5 text-left align-top font-semibold ${className ?? ""}`}
      >
        {children}
      </th>
    )
  },
  td({ className, children, ...props }: React.ComponentPropsWithoutRef<"td">) {
    return (
      <td
        {...props}
        className={`border border-[var(--glass-border)] px-2.5 py-1.5 align-top ${className ?? ""}`}
      >
        {children}
      </td>
    )
  },
}
