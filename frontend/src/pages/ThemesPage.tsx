import { toast } from "sonner";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { getMe, updateTheme } from "@/api";
import themes from "@/themes.json";
import { useBrowserState } from "@/hooks/useBrowserState";

export function ThemesPage() {
  const qc = useQueryClient();
  const { data: me } = useQuery({ queryKey: ["me"], queryFn: getMe });
  const [, setThm] = useBrowserState("theme", "catppuccin-latte");

  const themeMutation = useMutation({
    mutationFn: (themeId: string) => {
      setThm(themeId);
      return updateTheme(themeId);
    },
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: ["me"] });
    },
    onError: () => toast.error("failed to update theme"),
  });

  return (
    <div className="space-y-5 mt-0.5">
      <h2 className="text-2xl font-semibold">themes</h2>
      {Array.from(new Set(themes.map((t) => t.category))).map((category) => (
        <div key={category}>
          <p className="text-xs text-muted-foreground font-medium mb-2 lowercase">
            {category}
          </p>
          <div className="grid grid-cols-2 gap-2 sm:grid-cols-4">
            {themes
              .filter((t) => t.category === category)
              .map((t) => (
                <button
                  key={t.id}
                  onClick={() => themeMutation.mutate(t.id)}
                  disabled={themeMutation.isPending}
                  style={{
                    backgroundColor: t.preview[0],
                    borderColor:
                      me?.theme === t.id ? t.preview[2] : t.preview[3] + "44",
                  }}
                  className="text-left rounded-md border-2 px-3 py-2 text-sm transition-opacity hover:opacity-90 flex flex-row items-center justify-between"
                >
                  <span className="block" style={{ color: t.baseText }}>
                    {t.name}
                  </span>
                  <div className="flex flex-row-reverse justify-end">
                    {t.preview
                      .slice(1)
                      .toReversed()
                      .map((color) => (
                        <span
                          key={color}
                          className="block size-4 rounded-full -ml-1"
                          style={{
                            backgroundColor: color,
                          }}
                        />
                      ))}
                  </div>
                </button>
              ))}
          </div>
        </div>
      ))}
    </div>
  );
}
