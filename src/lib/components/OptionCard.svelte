<script lang="ts">
  import { Checkbox } from "$lib/components/ui/checkbox";
  import { Badge } from "$lib/components/ui/badge";

  interface Props {
    checked: boolean;
    label: string;
    desc?: string;
    onToggle: (checked: boolean) => void;
    badge?: string;
    badgeVariant?: "default" | "secondary" | "destructive" | "outline";
    disabled?: boolean;
  }

  let { checked, label, desc = "", onToggle, badge, badgeVariant = "secondary", disabled = false }: Props = $props();
</script>

<label
  class="flex items-center gap-3 rounded-lg border px-4 py-2.5 transition-colors
    {disabled ? 'opacity-40 cursor-not-allowed' : 'cursor-pointer'}
    {checked && !disabled ? 'border-primary/30 bg-primary/5' : 'border-border bg-muted/40' + (disabled ? '' : ' opacity-60')}"
>
  <Checkbox
    checked={checked}
    disabled={disabled}
    onCheckedChange={(v: boolean | "indeterminate") => onToggle(v === true)}
  />
  <div class="min-w-0 flex-1">
    <div class="text-[13px] font-medium {checked && !disabled ? '' : 'text-muted-foreground'}">{label}</div>
    {#if desc}
      <div class="text-[11px] text-muted-foreground truncate">{desc}</div>
    {/if}
  </div>
  {#if badge}
    <Badge variant={badgeVariant} class="text-[10px] shrink-0">{badge}</Badge>
  {/if}
</label>
