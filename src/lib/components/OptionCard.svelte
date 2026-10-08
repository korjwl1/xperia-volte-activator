<script lang="ts">
  import { Checkbox } from "$lib/components/ui/checkbox";
  import { Badge } from "$lib/components/ui/badge";
  import { LoaderCircle } from "@lucide/svelte/icons";

  interface Props {
    checked: boolean;
    label: string;
    desc?: string;
    onToggle: (checked: boolean) => void;
    badge?: string;
    badgeVariant?: "default" | "secondary" | "destructive" | "outline";
    disabled?: boolean;
    right?: string;
    /** 용량 실측 진행 중 — 우측에 스켈레톤 표시 */
    loading?: boolean;
  }

  let { checked, label, desc = "", onToggle, badge, badgeVariant = "secondary", disabled = false, right, loading = false }: Props = $props();
</script>

<label
  class="flex items-center gap-3 rounded-lg border px-4 py-2.5 transition-colors
    {disabled ? 'opacity-40 cursor-not-allowed' : 'cursor-pointer'}
    {checked && !disabled ? 'border-primary/30 bg-primary/5' : disabled ? 'border-border bg-muted/40' : 'border-border bg-card hover:bg-muted/40'}"
>
  <Checkbox
    checked={checked}
    disabled={disabled}
    onCheckedChange={(v: boolean | "indeterminate") => onToggle(v === true)}
  />
  <div class="min-w-0 flex-1">
    <div class="text-[13px] font-medium">{label}</div>
    {#if desc}
      <div class="text-[11px] leading-relaxed text-muted-foreground break-keep">{desc}</div>
    {/if}
  </div>
  {#if loading}
    <span class="inline-flex items-center gap-1 text-[11px] text-muted-foreground shrink-0" aria-label="용량 계산 중">
      <LoaderCircle size={12} class="animate-spin text-primary" />계산 중
    </span>
  {:else if right}
    <span class="text-[11px] text-muted-foreground font-mono shrink-0">{right}</span>
  {/if}
  {#if badge}
    <Badge variant={badgeVariant} class="text-[10px] shrink-0">{badge}</Badge>
  {/if}
</label>
