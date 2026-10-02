import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { downloadDir } from "@tauri-apps/api/path";
import { open, save } from "@tauri-apps/plugin-dialog";
import type { Analysis, DayRow, ItemRule, Report, Summary, TodayPlan } from "./types";

const app = document.querySelector<HTMLElement>("#app")!;
const ruleOptions: Array<{ value: ItemRule; label: string }> = [
  { value: "general", label: "일반 근무" },
  { value: "bonus4h", label: "4시간 가산" },
  { value: "leave8h", label: "8시간 인정(연차)" },
  { value: "deduct_requested", label: "근태신청 시간만큼 추가 차감" },
  { value: "exclude", label: "계산 제외" },
];

let currentPath: string | null = null;
let report: Report | null = null;
let unknownItems: string[] = [];
let storedRules: Record<string, ItemRule> = {};
let activeModal: "unknown" | "rules" | null = null;
let errorMessage = "";
let successMessage = "";
let loading = false;
let arrivalValue = "";
let bonusChoice = false;
let todayPlan: TodayPlan | null = null;

function escapeHtml(value: string): string {
  return value.replace(/[&<>"']/g, (character) => {
    const entities: Record<string, string> = {
      "&": "&amp;",
      "<": "&lt;",
      ">": "&gt;",
      '"': "&quot;",
      "'": "&#39;",
    };
    return entities[character];
  });
}

function errorText(error: unknown): string {
  if (typeof error === "string") return error;
  return error instanceof Error ? error.message : String(error);
}

function fileName(path: string | null): string {
  if (!path) return "";
  const segments = path.split(/[\\/]/);
  return segments[segments.length - 1] || path;
}

function formatHm(minutes: number): string {
  const sign = minutes < 0 ? "-" : "";
  const absolute = Math.abs(minutes);
  return `${sign}${Math.floor(absolute / 60)}:${String(absolute % 60).padStart(2, "0")}`;
}

function formatKorean(minutes: number): string {
  const sign = minutes < 0 ? "−" : "";
  const absolute = Math.abs(minutes);
  return `${sign}${Math.floor(absolute / 60)}시간 ${absolute % 60}분`;
}

function durationCell(minutes: number | null): string {
  return minutes === null ? "-" : formatHm(minutes);
}

function kindLabel(kind: DayRow["kind"]): string {
  switch (kind) {
    case "work": return "근무";
    case "leave": return "연차";
    case "inProgress": return "근무 중";
    case "incomplete": return "미완료";
    case "excluded": return "제외";
  }
}

function countedRows(rows: DayRow[]): DayRow[] {
  return rows.filter((row) => row.kind === "work" || row.kind === "leave");
}

function banner(): string {
  if (errorMessage) return `<div class="banner error" role="alert">${escapeHtml(errorMessage)}</div>`;
  if (successMessage) return `<div class="banner success" role="status">${escapeHtml(successMessage)}</div>`;
  if (loading) return `<div class="banner loading" role="status">엑셀 파일을 분석하고 있습니다…</div>`;
  return "";
}

function renderDropZone(): string {
  return `<section class="drop-zone" id="drop-zone" aria-label="엑셀 파일 놓기">
    <span class="drop-icon" aria-hidden="true">⇧</span>
    <span>detailGrid_excel_*.xlsx 파일을 여기로 끌어다 놓으세요</span>
  </section>`;
}

function renderEmpty(): string {
  return `<section class="empty-state">
    <div class="empty-mark" aria-hidden="true">근태</div>
    <h2>월별 근무시간을 계산하세요</h2>
    <p>아마란스에서 내려받은 출결 상세 엑셀 파일을 열거나 끌어다 놓으세요.</p>
  </section>`;
}

function renderSummary(summary: Summary): string {
  const diffClass = summary.diffMin >= 0 ? "positive" : "negative";
  const diffText = summary.diffMin > 0
    ? `+${formatKorean(summary.diffMin)}`
    : summary.diffMin < 0
      ? formatKorean(summary.diffMin)
      : formatKorean(0);
  return `<section class="summary-grid" aria-label="월간 요약">
    <article class="summary-card">
      <span class="summary-label">근무일수</span>
      <strong>${summary.workDays}일</strong>
      <span class="summary-detail">실근무 ${summary.actualDays}일 + 연차 ${summary.leaveDays}일</span>
    </article>
    <article class="summary-card">
      <span class="summary-label">기준 근무시간</span>
      <strong>${escapeHtml(formatKorean(summary.targetMin))}</strong>
    </article>
    <article class="summary-card">
      <span class="summary-label">실 근무시간</span>
      <strong>${escapeHtml(formatKorean(summary.totalMin))}</strong>
    </article>
    <article class="summary-card ${diffClass}">
      <span class="summary-label">부족/초과</span>
      <strong>${escapeHtml(diffText)}</strong>
    </article>
  </section>`;
}

function renderReport(report: Report): string {
  const counted = countedRows(report.rows);
  const workedTotal = counted.reduce((sum, row) => sum + (row.workedMin ?? 0), 0);
  const breakTotal = counted.reduce((sum, row) => sum + row.breakMin, 0);
  const bonusTotal = counted.reduce((sum, row) => sum + row.bonusMin, 0);
  const finalTotal = counted.reduce((sum, row) => sum + (row.finalMin ?? 0), 0);
  const rows = report.rows.map((row) => {
    const notCounted = row.kind === "inProgress" || row.kind === "incomplete" || row.kind === "excluded";
    const cells = [
      row.date,
      row.weekday,
      row.item || "-",
      row.actualIn ?? "-",
      row.recognizedIn ?? "-",
      row.out ?? "-",
      durationCell(row.workedMin),
      formatHm(row.breakMin),
      formatHm(row.bonusMin),
      row.finalMin === null ? "제외" : formatHm(row.finalMin),
      row.note || "-",
    ];
    return `<tr class="${notCounted ? "not-counted" : ""}">
      <td>${escapeHtml(cells[0])}<span class="kind-pill">${kindLabel(row.kind)}</span></td>
      <td>${escapeHtml(cells[1])}</td>
      <td>${escapeHtml(cells[2])}</td>
      <td>${escapeHtml(cells[3])}</td>
      <td>${escapeHtml(cells[4])}</td>
      <td>${escapeHtml(cells[5])}</td>
      <td class="duration">${escapeHtml(cells[6])}</td>
      <td class="duration">${escapeHtml(cells[7])}</td>
      <td class="duration">${escapeHtml(cells[8])}</td>
      <td class="duration final-cell">${escapeHtml(cells[9])}</td>
      <td class="note-cell">${escapeHtml(cells[10])}</td>
    </tr>`;
  }).join("");
  const arrival = report.inProgressArrival ?? arrivalValue;
  return `<section class="report-section">
    <div class="report-heading">
      <div><p class="eyebrow">${escapeHtml(fileName(currentPath))}</p><h2>${escapeHtml(report.period)} 근무시간</h2></div>
      <button class="primary" id="save-button" type="button">엑셀로 저장</button>
    </div>
    ${renderSummary(report.summary)}
    <section class="table-card" aria-label="일별 근무시간">
      <div class="table-scroll"><table>
        <thead><tr><th>날짜</th><th>요일</th><th>근태항목</th><th>실출근</th><th>인정출근</th><th>퇴근</th><th>인정근무(휴게 전)</th><th>휴게차감</th><th>반차가산</th><th>최종근무시간</th><th>비고</th></tr></thead>
        <tbody>${rows}
          <tr class="total-row"><th colspan="6">합계</th><td class="duration">${formatHm(workedTotal)}</td><td class="duration">${formatHm(breakTotal)}</td><td class="duration">${formatHm(bonusTotal)}</td><td class="duration">${formatHm(finalTotal)}</td><td></td></tr>
        </tbody>
      </table></div>
    </section>
    <div class="lower-grid">
      <section class="panel today-panel">
        <div class="panel-heading"><div><p class="eyebrow">오늘의 계산</p><h3>오늘 퇴근 목표</h3></div></div>
        <div class="today-fields">
          <label>출근 시각<input id="today-arrival" type="time" value="${escapeHtml(arrival)}" /></label>
          <label>오늘 근태<select id="today-bonus">
            <option value="general" ${bonusChoice ? "" : "selected"}>일반 근무</option>
            <option value="bonus" ${bonusChoice ? "selected" : ""}>4시간 가산 (반차·생일휴가·단축·건강검진)</option>
          </select></label>
        </div>
        <button class="secondary" id="today-calculate" type="button">계산</button>
        ${todayPlan ? renderTodayPlan(todayPlan) : ""}
      </section>
      <section class="panel notes-panel">
        <div class="panel-heading"><div><p class="eyebrow">적용된 기준</p><h3>비고</h3></div></div>
        <ul>${report.notes.map((note) => `<li>${escapeHtml(note)}</li>`).join("")}</ul>
      </section>
    </div>
  </section>`;
}

function renderTodayPlan(plan: TodayPlan): string {
  const result = plan.alreadyMet
    ? `<strong>이미 기준을 충족했습니다 (오늘 추가 근무 불필요)</strong>`
    : `<strong>퇴근 목표 ${escapeHtml(plan.leaveAt ?? "")}</strong>
      <span>(인정출근 ${escapeHtml(plan.recognizedIn)}, 필요 실근무 ${escapeHtml(formatKorean(plan.workNeededMin))}${plan.breakApplied ? ", 휴게 1시간 포함" : ""})</span>`;
  return `<div class="today-result" role="status">${result}${plan.halfDayNotice ? `<p>반차일은 14:00 이전에 퇴근하면 휴게 1시간이 차감되지 않습니다.</p>` : ""}</div>`;
}

function renderModal(): string {
  if (activeModal === "unknown") {
    const groups = unknownItems.map((item, index) => `<fieldset class="rule-choice">
      <legend>${escapeHtml(item)}</legend>
      <div class="radio-grid">${ruleOptions.map((option) => `<label class="radio-option">
        <input type="radio" name="unknown-${index}" value="${option.value}" ${option.value === "general" ? "checked" : ""} />
        <span>${escapeHtml(option.label)}</span>
      </label>`).join("")}</div>
    </fieldset>`).join("");
    return `<div class="modal-backdrop"><section class="modal" role="dialog" aria-modal="true" aria-labelledby="modal-title">
      <h2 id="modal-title">처음 보는 근태항목이 있습니다. 처리 방식을 선택하세요.</h2>
      <div class="modal-content">${groups}</div>
      <div class="modal-actions"><button class="secondary" id="modal-cancel" type="button">취소</button><button class="primary" id="rules-apply" type="button">적용</button></div>
    </section></div>`;
  }
  if (activeModal === "rules") {
    const entries = Object.entries(storedRules);
    const list = entries.length === 0
      ? `<p class="empty-rules">저장된 사용자 지정 규칙이 없습니다</p>`
      : `<ul class="saved-rules">${entries.map(([item, rule]) => `<li><span><strong>${escapeHtml(item)}</strong><small>${escapeHtml(ruleOptions.find((option) => option.value === rule)?.label ?? rule)}</small></span><button class="delete-rule" data-delete-rule="${escapeHtml(item)}" type="button">삭제</button></li>`).join("")}</ul>`;
    return `<div class="modal-backdrop"><section class="modal rules-dialog" role="dialog" aria-modal="true" aria-labelledby="modal-title">
      <h2 id="modal-title">규칙 설정</h2><div class="modal-content">${list}</div>
      <div class="modal-actions"><button class="primary" id="modal-close" type="button">닫기</button></div>
    </section></div>`;
  }
  return "";
}

function render(): void {
  app.innerHTML = `<div class="app-shell">
    <header class="topbar">
      <div class="brand"><span class="brand-mark">A</span><div><h1>근무시간 계산기</h1><p>아마란스 출결 엑셀 월간 정리</p></div></div>
      <nav class="header-actions" aria-label="파일 및 규칙">
        <button class="primary" id="open-button" type="button">엑셀 파일 열기</button>
        <button class="secondary" id="rules-button" type="button">규칙 설정</button>
      </nav>
    </header>
    ${banner()}
    ${renderDropZone()}
    ${report ? renderReport(report) : renderEmpty()}
    ${renderModal()}
    <footer>파일은 이 기기에서만 처리됩니다. 개인 출결 엑셀 파일은 저장소에 포함되지 않습니다.</footer>
  </div>`;
  bindControls();
}

function setError(error: unknown): void {
  errorMessage = errorText(error);
  successMessage = "";
  loading = false;
  render();
}

async function analyzeCurrent(path: string): Promise<void> {
  currentPath = path;
  report = null;
  unknownItems = [];
  activeModal = null;
  todayPlan = null;
  arrivalValue = "";
  errorMessage = "";
  successMessage = "";
  loading = true;
  render();
  try {
    const analysis = await invoke<Analysis>("analyze_file", { path });
    loading = false;
    if (analysis.status === "needsRules") {
      unknownItems = analysis.unknownItems;
      activeModal = "unknown";
      render();
      return;
    }
    report = analysis.report;
    arrivalValue = report.inProgressArrival ?? "";
    render();
  } catch (error) {
    setError(error);
  }
}

async function chooseFile(): Promise<void> {
  errorMessage = "";
  successMessage = "";
  try {
    const directory = await downloadDir().catch(() => undefined);
    const selected = await open({
      multiple: false,
      filters: [{ name: "Excel", extensions: ["xlsx"] }],
      ...(directory ? { defaultPath: directory } : {}),
    });
    if (typeof selected === "string") await analyzeCurrent(selected);
  } catch (error) {
    setError(error);
  }
}

async function saveReport(): Promise<void> {
  if (!currentPath || !report) return;
  errorMessage = "";
  successMessage = "";
  try {
    const output = await save({
      defaultPath: `근무시간_${report.period}.xlsx`,
      filters: [{ name: "Excel", extensions: ["xlsx"] }],
    });
    if (!output) return;
    await invoke("export_report", { path: currentPath, outPath: output });
    successMessage = `저장했습니다: ${output}`;
    render();
  } catch (error) {
    setError(error);
  }
}

async function showRules(): Promise<void> {
  errorMessage = "";
  successMessage = "";
  try {
    storedRules = await invoke<Record<string, ItemRule>>("list_item_rules");
    activeModal = "rules";
    render();
  } catch (error) {
    setError(error);
  }
}

async function applyUnknownRules(): Promise<void> {
  const rules: Record<string, ItemRule> = {};
  unknownItems.forEach((_item, index) => {
    const selected = document.querySelector<HTMLInputElement>(`input[name="unknown-${index}"]:checked`);
    if (selected) rules[unknownItems[index]] = selected.value as ItemRule;
  });
  if (Object.keys(rules).length !== unknownItems.length) {
    errorMessage = "모든 근태항목의 처리 방식을 선택하세요.";
    render();
    return;
  }
  try {
    await invoke("set_item_rules", { rules });
    activeModal = null;
    if (currentPath) await analyzeCurrent(currentPath);
  } catch (error) {
    setError(error);
  }
}

async function deleteRule(item: string): Promise<void> {
  try {
    await invoke("delete_item_rule", { item });
    if (currentPath) {
      activeModal = null;
      await analyzeCurrent(currentPath);
    } else {
      storedRules = await invoke<Record<string, ItemRule>>("list_item_rules");
      render();
    }
  } catch (error) {
    setError(error);
  }
}

async function calculateToday(): Promise<void> {
  if (!report) return;
  const arrival = document.querySelector<HTMLInputElement>("#today-arrival")?.value ?? "";
  const bonus = document.querySelector<HTMLSelectElement>("#today-bonus")?.value === "bonus";
  arrivalValue = arrival;
  bonusChoice = bonus;
  try {
    todayPlan = await invoke<TodayPlan>("plan_today", {
      summary: report.summary,
      arrival,
      bonus,
    });
    errorMessage = "";
    render();
  } catch (error) {
    setError(error);
  }
}

function bindControls(): void {
  document.querySelector("#open-button")?.addEventListener("click", () => void chooseFile());
  document.querySelector("#rules-button")?.addEventListener("click", () => void showRules());
  document.querySelector("#save-button")?.addEventListener("click", () => void saveReport());
  document.querySelector("#today-calculate")?.addEventListener("click", () => void calculateToday());
  document.querySelector("#rules-apply")?.addEventListener("click", () => void applyUnknownRules());
  document.querySelector("#modal-cancel")?.addEventListener("click", () => {
    activeModal = null;
    unknownItems = [];
    errorMessage = "";
    render();
  });
  document.querySelector("#modal-close")?.addEventListener("click", () => {
    activeModal = null;
    render();
  });
  document.querySelectorAll<HTMLButtonElement>("[data-delete-rule]").forEach((button) => {
    button.addEventListener("click", () => void deleteRule(button.dataset.deleteRule ?? ""));
  });
}

render();
void getCurrentWebview().onDragDropEvent((event) => {
  if (event.payload.type !== "drop") return;
  const path = event.payload.paths[0];
  if (!path) return;
  if (!path.toLowerCase().endsWith(".xlsx")) {
    errorMessage = "xlsx 파일만 지원합니다";
    successMessage = "";
    render();
    return;
  }
  void analyzeCurrent(path);
});
