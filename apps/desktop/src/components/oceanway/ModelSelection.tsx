import { useState } from "react";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";

export function ModelSelection({
  label,
  choices,
  selected,
  defaultModel,
  disabled,
  onChange,
}: {
  label: string;
  choices: string[];
  selected: string[];
  defaultModel: string;
  disabled: boolean;
  onChange: (models: string[], defaultModel: string) => void;
}) {
  const [search, setSearch] = useState("");
  const visible = choices.filter((id) =>
    id.toLowerCase().includes(search.trim().toLowerCase()),
  );
  function update(next: string[]) {
    onChange(
      next,
      next.includes(defaultModel) ? defaultModel : (next[0] ?? ""),
    );
  }
  return (
    <div className="space-y-3" role="group" aria-label={`${label}接入模型`}>
      <div className="flex flex-wrap items-center justify-between gap-2">
        <span className="text-caption text-fg-2">
          勾选要接入的模型 · 已选 {selected.length} 个
        </span>
        <Button
          variant="quiet"
          size="compact"
          disabled={disabled || !visible.length}
          onClick={() => update([...new Set([...selected, ...visible])])}
        >
          全选当前列表
        </Button>
      </div>
      {choices.length > 5 && (
        <Input
          aria-label={`搜索${label}模型`}
          placeholder="搜索模型"
          value={search}
          disabled={disabled}
          onChange={(event) => setSearch(event.target.value)}
        />
      )}
      <div className="max-h-60 space-y-1 overflow-y-auto rounded-control border border-border bg-surface p-2">
        {visible.map((id) => (
          <label
            key={id}
            className="flex cursor-pointer items-center gap-3 rounded-control px-2 py-2 hover:bg-subtle"
          >
            <Checkbox
              aria-label={`接入 ${label} ${id}`}
              checked={selected.includes(id)}
              disabled={
                disabled || (selected.length === 1 && selected.includes(id))
              }
              onCheckedChange={(checked) =>
                update(
                  checked
                    ? [...new Set([...selected, id])]
                    : selected.filter((model) => model !== id),
                )
              }
            />
            <span className="min-w-0 break-all text-body">{id}</span>
            {id === defaultModel && (
              <span className="ml-auto shrink-0 text-caption text-fg-3">
                默认
              </span>
            )}
          </label>
        ))}
        {!visible.length && (
          <p className="p-2 text-caption text-fg-3">没有匹配的模型</p>
        )}
      </div>
      <label className="flex flex-wrap items-center gap-2 text-caption text-fg-2">
        <span className="w-16 shrink-0">默认模型</span>
        <select
          aria-label={`${label}默认模型`}
          value={defaultModel}
          disabled={disabled || !selected.length}
          onChange={(event) => onChange(selected, event.target.value)}
          className="h-9 min-w-0 flex-1 rounded-control border border-input bg-surface px-2 text-body"
        >
          {!selected.length && <option value="">请先选择模型</option>}
          {selected.map((id) => (
            <option key={id} value={id}>
              {id}
            </option>
          ))}
        </select>
      </label>
    </div>
  );
}
