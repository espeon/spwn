import { Toaster as Sonner, type ToasterProps } from "sonner"

export function Toaster(props: ToasterProps) {
  return (
    <Sonner
      className="toaster group"
      style={
        {
          "--normal-bg": "var(--popover)",
          "--normal-border": "var(--border)",
          "--normal-text": "var(--popover-foreground)",
          "--error-bg": "var(--popover)",
          "--error-border": "var(--destructive)",
          "--error-text": "var(--destructive)",
          "--success-bg": "var(--popover)",
          "--success-border": "var(--primary)",
          "--success-text": "var(--primary)",
        } as React.CSSProperties
      }
      {...props}
    />
  )
}
