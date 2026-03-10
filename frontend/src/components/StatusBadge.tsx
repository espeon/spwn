import { Badge } from "@/components/ui/badge";
import type { VmStatus } from "@/api";

const variantMap: Record<
  VmStatus,
  {
    variant: "default" | "secondary" | "destructive" | "outline";
    className: string;
  }
> = {
  running: {
    variant: "outline",
    className: "border-green-800 text-green-400/80",
  },
  starting: {
    variant: "outline",
    className: "border-yellow-800 text-yellow-400",
  },
  stopping: {
    variant: "outline",
    className: "border-yellow-800 text-yellow-400",
  },
  stopped: {
    variant: "outline",
    className: "border-border text-muted-foreground",
  },
  snapshotting: {
    variant: "outline",
    className: "border-blue-800 text-blue-400",
  },
  error: { variant: "outline", className: "border-red-800 text-red-400" },
};

export function StatusBadge({ status }: { status: VmStatus }) {
  const { variant, className } = variantMap[status];
  return (
    <Badge variant={variant} className={`font-mono font-normal ${className}`}>
      {status}
    </Badge>
  );
}
