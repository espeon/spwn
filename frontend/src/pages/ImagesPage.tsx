import { useMemo } from "react";
import { useQuery } from "@tanstack/react-query";
import { listImages, type Image } from "@/api";
import { IconBox } from "@tabler/icons-react";
import { formatDataSize } from "@/lib/utils";

function statusColor(status: string): string {
  switch (status) {
    case "ready":
      return "text-green-500";
    case "building":
      return "text-yellow-500";
    case "error":
      return "text-destructive";
    default:
      return "text-muted-foreground";
  }
}

function timeAgo(ts: number): string {
  const diff = Math.max(0, Math.floor(Date.now() / 1000 - ts));
  if (diff < 60) return `${diff}s ago`;
  if (diff < 3600) return `${Math.floor(diff / 60)}m ago`;
  if (diff < 86400) return `${Math.floor(diff / 3600)}h ago`;
  return `${Math.floor(diff / 86400)}d ago`;
}

function ImageCard({ image }: { image: Image }) {
  const age = useMemo(() => timeAgo(image.created_at), [image.created_at]);

  return (
    <div className="rounded-lg border bg-card px-5 py-4 flex flex-row items-center justify-between">
      <div className="flex items-center gap-4 min-w-0">
        <IconBox className="size-8 shrink-0 text-muted-foreground opacity-60" />
        <div className="min-w-0">
          <div className="flex items-center gap-2">
            <span className="font-medium">
              {image.name}
            </span>
            <span className="text-xs font-mono bg-secondary px-1.5 py-0.5 rounded text-muted-foreground">
              {image.tag}
            </span>
            <span className={`text-xs font-medium ${statusColor(image.status)}`}>
              {image.status}
            </span>
          </div>
          <p className="text-xs text-muted-foreground mt-0.5 font-mono truncate">
            {image.source}
          </p>
        </div>
      </div>
      <div className="flex items-center gap-6 shrink-0 text-xs text-muted-foreground text-right">
        <div>
          <p className="font-medium text-foreground">
            {image.size_bytes > 0 ? formatDataSize(image.size_bytes) : "—"}
          </p>
          <p>size</p>
        </div>
        <div>
          <p className="font-medium text-foreground">{age}</p>
          <p>built</p>
        </div>
      </div>
    </div>
  );
}

export function ImagesPage() {
  const { data: images = [], isLoading, error } = useQuery({
    queryKey: ["images"],
    queryFn: listImages,
  });

  if (isLoading) {
    return <p className="text-muted-foreground text-sm">loading...</p>;
  }

  if (error) {
    return <p className="text-destructive text-sm">failed to load images</p>;
  }

  return (
    <div className="flex flex-col gap-6">
      <div className="flex items-start justify-between">
        <div>
          <h1 className="text-xl font-semibold">image catalogue</h1>
          <p className="text-sm text-muted-foreground mt-1">
            base images available for new VMs
          </p>
        </div>
        <div className="text-right">
          <p className="text-sm font-medium">{images.length}</p>
          <p className="text-xs text-muted-foreground">
            image{images.length !== 1 ? "s" : ""}
          </p>
        </div>
      </div>

      {images.length === 0 ? (
        <div className="flex flex-col items-center gap-2 py-24 text-muted-foreground">
          <IconBox className="size-10 opacity-30" />
          <p className="text-sm">no images available yet</p>
          <p className="text-xs">an admin can build images from the admin panel</p>
        </div>
      ) : (
        <div className="flex flex-col gap-2">
          {images.map((img) => (
            <ImageCard key={img.id} image={img} />
          ))}
        </div>
      )}
    </div>
  );
}
