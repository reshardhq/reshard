import { Toaster as Sonner, type ToasterProps } from "sonner"
import {
  BadgeCheckIcon,
  InfoIcon,
  CircleAlertIcon,
  LoaderCircleIcon,
} from "@/components/ui/icons"

const Toaster = ({ ...props }: ToasterProps) => {
  return (
    <Sonner
      theme="dark"
      position="top-center"
      className="toaster group"
      icons={{
        success: <BadgeCheckIcon className="size-5" />,
        info: <InfoIcon className="size-5" />,
        warning: <CircleAlertIcon className="size-5" />,
        error: <CircleAlertIcon className="size-5" />,
        loading: <LoaderCircleIcon className="size-5 animate-spin" />,
      }}
      style={
        {
          "--normal-bg": "var(--popover)",
          "--normal-text": "var(--popover-foreground)",
          "--normal-border": "var(--border)",
          "--border-radius": "var(--radius)",
        } as React.CSSProperties
      }
      toastOptions={{
        classNames: {
          toast: "cn-toast",
        },
      }}
      {...props}
    />
  )
}

export { Toaster }
