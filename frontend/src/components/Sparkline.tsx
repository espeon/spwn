interface SparklineProps {
  values: number[];
  label: string;
  current: string;
  height?: number;
  color?: string;
}

export function Sparkline({
  values,
  label,
  current,
  height = 36,
  color = "currentColor",
}: SparklineProps) {
  const width = 120;
  const pad = 2;

  const points = values.length < 2 ? [] : (() => {
    const min = Math.min(...values);
    const max = Math.max(...values);
    const range = max - min || 1;
    return values.map((v, i) => {
      const x = pad + (i / (values.length - 1)) * (width - pad * 2);
      const y = height - pad - ((v - min) / range) * (height - pad * 2);
      return `${x.toFixed(1)},${y.toFixed(1)}`;
    });
  })();

  return (
    <div>
      <p className="text-xs text-muted-foreground mb-1">{label}</p>
      <p className="font-mono text-sm mb-1">{current}</p>
      <svg
        width="100%"
        height={height}
        viewBox={`0 0 ${width} ${height}`}
        preserveAspectRatio="none"
        className="opacity-70"
      >
        {points.length > 0 && (
          <polyline
            points={points.join(" ")}
            fill="none"
            stroke={color}
            strokeWidth="1.5"
            strokeLinejoin="round"
            strokeLinecap="round"
          />
        )}
      </svg>
    </div>
  );
}
