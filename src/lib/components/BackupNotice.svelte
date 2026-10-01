<script lang="ts">
  // 백업 직전 안내 — 복구되지 않는 항목(기본 포커스) / 백업될 설정 / 백업될 앱을 보여주고 동의를 받는다.
  import { Button } from "$lib/components/ui/button";
  import { Checkbox } from "$lib/components/ui/checkbox";
  import { CircleCheck, OctagonX, LoaderCircle, ShieldAlert, KeyRound } from "@lucide/svelte/icons";
  import { wizard } from "$lib/stores/wizard.svelte";
  import type { AppItem } from "$lib/types";

  let tab = $state<"none" | "settings" | "apps">("none");

  // 앱 데이터 외에 이 프로그램으로 백업·복구할 수 없는 항목 (recovery.md 1-1 ③, 1-4)
  const NON_RECOVERABLE = [
    { title: "공동인증서 · 금융인증서 · OTP", desc: "각 앱의 내보내기/이전 기능으로 미리 옮겨 두지 않으면 재발급해야 합니다" },
    { title: "간편결제 · 교통카드 · 은행 앱 기기 등록", desc: "초기화 후 기기 재등록과 본인 인증이 필요합니다" },
    { title: "지문 · 얼굴 인식, 화면 잠금", desc: "보안상 백업되지 않으며 다시 등록해야 합니다" },
    { title: "앱별 알림 설정", desc: "각 앱의 알림 켜기/끄기를 다시 설정해야 합니다" },
    { title: "기본 앱 지정", desc: "기본 브라우저 · 전화 · 문자 앱을 다시 지정해야 합니다" },
    { title: "기본 앱 내부 설정", desc: "카메라 · 동영상 편집 등 Sony 기본 앱의 설정은 복원되지 않습니다" },
    { title: "블루투스 기기 연결", desc: "구글 백업으로 일부 복원되며, 안 되는 기기는 다시 페어링해야 합니다" },
    { title: "SIM PIN", desc: "설정해 둔 SIM 잠금을 다시 설정해야 합니다" },
  ];

  const checked = (id: string) => wizard.groups.some((g) => g.items.some((i) => i.id === id && i.checked));
  const settingsChecked = $derived(checked("settings-all"));
  const apkChecked = $derived(checked("apk"));
  const appDataChecked = $derived(checked("app-data"));

  // 분류(data/appRules.ts) + 이번 백업 선택을 반영한 실제 결과
  const byLabel = (a: AppItem, b: AppItem) => a.label.localeCompare(b.label);
  const apps = $derived(wizard.appClasses ?? []);
  const lostApps = $derived(apps.filter((a) => a.recovery === "lost").sort(byLabel));
  // 앱 데이터 백업을 선택하지 않으면 외부 데이터가 있어도 복원되지 않으므로 재로그인 쪽으로
  const reloginApps = $derived(
    apps.filter((a) => a.recovery === "relogin" || (a.recovery === "restored" && !appDataChecked)).sort(byLabel),
  );
  const restoredApps = $derived(appDataChecked ? apps.filter((a) => a.recovery === "restored").sort(byLabel) : []);

  /** 설정 값 → 사람이 읽는 문구 */
  function fmtSetting(key: string, value: string): string {
    if (value === "null" || value === "") return "기본값";
    switch (key) {
      case "sysui_qs_tiles": return `${value.split(",").length}개 타일`;
      case "default_input_method": return value.split("/")[0];
      case "screen_brightness": return `${Math.round((Number(value) / 255) * 100)}%`;
      case "screen_brightness_mode": return value === "1" ? "켜짐" : "꺼짐";
      case "screen_off_timeout": {
        const s = Number(value) / 1000;
        return s >= 60 ? `${Math.round(s / 60)}분` : `${s}초`;
      }
      case "font_scale": return Number(value) === 1 ? "기본 (1.0배)" : `${value}배`;
      case "stay_on_while_plugged_in": return value === "0" ? "꺼짐" : "켜짐";
      default: return value;
    }
  }

  const tabs = [
    { id: "none", label: "백업 및 복구 불가능" },
    { id: "settings", label: "설정" },
    { id: "apps", label: "앱" },
  ] as const;
</script>

<div class="fixed inset-0 z-50 flex items-center justify-center bg-black/70 backdrop-blur-sm p-4" role="dialog">
  <div class="w-full max-w-3xl h-[85vh] rounded-2xl border bg-background elev-3 flex flex-col overflow-hidden">
    <!-- 헤더 -->
    <div class="shrink-0 px-6 pt-5 pb-3 flex items-start gap-3">
      <span class="flex h-10 w-10 shrink-0 items-center justify-center rounded-xl bg-warning-container text-warning">
        <ShieldAlert size={20} />
      </span>
      <div class="space-y-0.5">
        <h2 class="text-base font-semibold">백업을 시작하기 전에 확인해 주세요</h2>
        <p class="text-xs text-muted-foreground">
          이후 단계에서 휴대폰이 초기화됩니다. 아래 항목은 이 프로그램으로 복구되지 않거나 일부만 복구됩니다.
        </p>
      </div>
    </div>

    <!-- 탭 -->
    <div class="shrink-0 px-6">
      <div class="flex gap-1 rounded-lg bg-muted p-1">
        {#each tabs as t (t.id)}
          <button
            class="flex-1 rounded-md px-4 py-2 text-sm font-medium transition-colors
              {tab === t.id ? 'bg-background elev-1 text-foreground' : 'text-muted-foreground hover:text-foreground'}"
            onclick={() => (tab = t.id)}
          >
            {t.label}
          </button>
        {/each}
      </div>
    </div>

    <!-- 내용 (자체 스크롤) -->
    <div class="flex-1 min-h-0 overflow-y-auto px-6 py-4 space-y-5">
      {#if tab === "none"}
        {#if wizard.appClassesState === "loading"}
          <div class="flex items-center gap-2 text-xs text-muted-foreground"><LoaderCircle size={13} class="animate-spin text-primary" />앱 목록 확인 중…</div>
        {:else if wizard.appClassesState === "failed"}
          <div class="text-xs text-muted-foreground">앱 목록을 확인할 수 없습니다</div>
        {:else}
          {#if lostApps.length > 0}
            <section class="space-y-2">
              <div class="text-xs font-semibold text-destructive">미리 직접 옮기지 않으면 데이터가 사라지는 앱 ({lostApps.length})</div>
              <div class="rounded-lg border border-destructive/30 divide-y">
                {#each lostApps as app (app.pkg)}
                  <div class="flex items-center gap-2.5 px-3 py-2">
                    <OctagonX size={14} class="shrink-0 text-destructive" />
                    <div class="min-w-0 flex-1">
                      <div class="text-[12.5px] truncate" title={app.pkg}>{app.label}</div>
                      <div class="text-[11px] text-muted-foreground" title={app.note}>{app.note}</div>
                    </div>
                  </div>
                {/each}
              </div>
            </section>
          {/if}
          <section class="space-y-2">
            <div class="text-xs font-semibold text-muted-foreground">다시 로그인이 필요한 앱 ({reloginApps.length})</div>
            <p class="text-[11px] text-muted-foreground">
              {apkChecked
                ? "앱은 백업한 APK로 다시 설치되지만 앱 안의 데이터는 복원되지 않습니다. 계정에 저장된 데이터는 다시 로그인하면 돌아옵니다."
                : "앱(APK) 백업을 선택하지 않아 앱을 직접 다시 설치하고 로그인해야 합니다."}
            </p>
            <div class="rounded-lg border divide-y">
              {#each reloginApps as app (app.pkg)}
                <div class="flex items-center gap-2.5 px-3 py-2">
                  <KeyRound size={14} class="shrink-0 text-warning" />
                  <div class="min-w-0 flex-1">
                    <div class="text-[12.5px] truncate" title={app.pkg}>{app.label}</div>
                    <div class="text-[11px] text-muted-foreground truncate" title={app.note}>{app.note}</div>
                  </div>
                </div>
              {/each}
            </div>
          </section>
        {/if}
        <section class="space-y-2">
          <div class="text-xs font-semibold text-muted-foreground">그 외 복구되지 않는 항목</div>
          <div class="rounded-lg border divide-y">
            {#each NON_RECOVERABLE as item (item.title)}
              <div class="px-3 py-2">
                <div class="text-[12.5px] font-medium">{item.title}</div>
                <div class="text-[11px] text-muted-foreground">{item.desc}</div>
              </div>
            {/each}
          </div>
        </section>

      {:else if tab === "settings"}
        {#if !settingsChecked}
          <div class="rounded-lg bg-warning-container/60 px-4 py-2 text-xs text-warning">설정 백업을 선택하지 않아 아래 설정은 백업되지 않습니다</div>
        {/if}
        {#if wizard.settingsInfoState === "loading"}
          <div class="flex items-center gap-2 text-xs text-muted-foreground"><LoaderCircle size={13} class="animate-spin text-primary" />설정 정보 확인 중…</div>
        {:else if !wizard.settingsInfo}
          <div class="text-xs text-muted-foreground">설정 정보를 확인할 수 없습니다</div>
        {:else}
          {@const s = wizard.settingsInfo}
          <section class="space-y-2">
            <div class="text-xs font-semibold text-muted-foreground">초기화 후 자동으로 되돌리는 설정</div>
            <div class="rounded-lg border divide-y">
              {#each s.restoreItems as it (it.key)}
                <div class="flex items-center gap-3 px-3 py-2">
                  <CircleCheck size={14} class="shrink-0 text-success" />
                  <span class="text-[12.5px] flex-1">{it.label}</span>
                  <span class="text-[11px] text-muted-foreground font-mono truncate max-w-[50%]" title={it.value}>{fmtSetting(it.key, it.value)}</span>
                </div>
              {/each}
              <div class="flex items-center gap-3 px-3 py-2">
                <CircleCheck size={14} class="shrink-0 text-success" />
                <span class="text-[12.5px] flex-1">배터리 최적화 예외 앱</span>
                <span class="text-[11px] text-muted-foreground font-mono">{s.batteryExemptApps}개</span>
              </div>
            </div>
          </section>
          <section class="space-y-1">
            <div class="text-xs font-semibold text-muted-foreground">보관용 전체 설정 기록</div>
            <p class="text-[12px]">
              시스템 {s.systemCount}개 · 보안 {s.secureCount}개 · 전역 {s.globalCount}개 설정 값을 기록으로 보관합니다.
            </p>
            <p class="text-[11px] text-muted-foreground">
              위 목록 외의 설정은 자동으로 되돌리지 않습니다 — Wi-Fi · 언어 · 다크 모드 등은 초기 설정 때 구글 백업에서 복원할 수 있습니다.
            </p>
          </section>
        {/if}

      {:else}
        <section class="space-y-2">
          <div class="text-xs font-semibold text-muted-foreground">
            백업·복구되는 앱 {wizard.appClasses && appDataChecked ? `(${restoredApps.length})` : ""}
          </div>
          {#if wizard.appClassesState === "loading"}
            <div class="flex items-center gap-2 text-xs text-muted-foreground"><LoaderCircle size={13} class="animate-spin text-primary" />앱 목록 확인 중…</div>
          {:else if wizard.appClassesState === "failed"}
            <div class="text-xs text-muted-foreground">앱 목록을 확인할 수 없습니다</div>
          {:else if !appDataChecked}
            <div class="rounded-lg bg-warning-container/60 px-4 py-2 text-xs text-warning">앱 데이터 백업을 선택하지 않아 복원되는 앱 데이터가 없습니다</div>
          {:else}
            <p class="text-[11px] text-muted-foreground">
              {apkChecked ? "백업한 APK로 다시 설치한 뒤" : "앱을 직접 다시 설치하면"} 앱 외부 데이터(Android/data — 받은 사진·파일, 다운로드한 콘텐츠 등)를 되돌립니다. 로그인은 다시 해야 할 수 있습니다.
            </p>
            <div class="rounded-lg border divide-y">
              {#each restoredApps as app (app.pkg)}
                <div class="flex items-center gap-2.5 px-3 py-2">
                  <CircleCheck size={14} class="shrink-0 text-success" />
                  <div class="min-w-0 flex-1">
                    <div class="text-[12.5px] truncate" title={app.pkg}>{app.label}</div>
                    <div class="text-[11px] text-muted-foreground truncate" title={app.note}>{app.note}</div>
                  </div>
                  {#if apkChecked}<span class="rounded border px-1.5 py-0.5 text-[10px] text-muted-foreground shrink-0">APK</span>{/if}
                  <span class="rounded border px-1.5 py-0.5 text-[10px] text-muted-foreground shrink-0">외부 데이터</span>
                </div>
              {/each}
            </div>
          {/if}
        </section>
      {/if}
    </div>

    <!-- 하단: 고지 + 동의 -->
    <div class="shrink-0 border-t bg-muted/40 px-6 py-4 space-y-3">
      <p class="text-[11.5px] leading-relaxed text-muted-foreground">
        백업은 휴대폰을 작업 전과 완전히 같은 상태로 되돌리는 것을 보장하지 않습니다.
        필요한 데이터는 각 앱의 백업 · 내보내기 기능이나 계정 동기화로 지금 미리 옮겨 두세요.
        사전 조치를 하지 않아 발생하는 데이터 손실에 대한 책임은 사용자에게 있습니다.
      </p>
      <div class="flex items-center gap-3">
        <label class="flex flex-1 items-center gap-2.5 cursor-pointer">
          <Checkbox checked={wizard.backupNoticeAck} onCheckedChange={(v: boolean | "indeterminate") => (wizard.backupNoticeAck = v === true)} />
          <span class="text-[13px] font-medium">필요한 사전 백업을 마쳤으며, 위 내용을 이해했습니다</span>
        </label>
        <Button variant="outline" size="sm" onclick={() => wizard.abort()}>작업 중단</Button>
        <Button size="sm" disabled={!wizard.backupNoticeAck} onclick={() => wizard.ackManual()}>백업 시작</Button>
      </div>
    </div>
  </div>
</div>
