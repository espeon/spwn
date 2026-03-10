import { cn } from "@/lib/utils"; // Standard shadcn/utils import

interface SpinnerProps {
  size?: number; // Size in pixels
  className?: string;
}

export const Loader = ({ size = 48, className }: SpinnerProps) => {
  // Calculated values to maintain aspect ratios from original CSS
  const bladeWidth = size / 12.0;
  const bladeHeight = size / 4.0;
  const radius = size / 2.0;

  return (
    <div
      className={cn("relative", className)}
      style={{ width: size, height: size }}
    >
      {Array.from({ length: 12 }).map((_, i) => {
        // Calculate rotation and delay for each blade
        const rotation = i * 30; // 360 / 12
        const delay = i * 0.083; // 1s / 12

        return (
          <div
            key={i}
            className="absolute left-1/2 top-1/2 animate-spinner-blade rounded-full bg-muted-foreground"
            style={{
              width: bladeWidth,
              height: bladeHeight,
              transform: `translate(-50%, -100%) rotate(${rotation}deg) translateY(-${radius - bladeHeight}px)`,
              transformOrigin: "center bottom",
              animationDelay: `${delay}s`,
            }}
          />
        );
      })}
    </div>
  );
};
