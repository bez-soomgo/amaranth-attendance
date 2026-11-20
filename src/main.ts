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

type LeaveKind = "annual" | "amHalf" | "pmHalf";

interface DraftSettings {
  leaveReason: string;
  clubName: string;
  cardName: string;
  attendees: string[];
  perPerson: number;
}

interface DraftStatus {
  id: number;
  state: "idle" | "opening" | "login" | "filling" | "awaitingReceipts" | "done" | "empty" | "failed" | "closed";
  step: string;
  message: string;
}

type GroupwareError =
  | { code: "loginRequired"; windowOpen: boolean }
  | { code: "failed"; message: string };

let currentPath: string | null = null;
let source: "file" | "groupware" | null = null;
let loginPoll: number | null = null;
let fetchInFlight = false;
let loginFailures = 0;
let report: Report | null = null;
let unknownItems: string[] = [];
let storedRules: Record<string, ItemRule> = {};
let activeModal: "unknown" | "rules" | "credentials" | "draftChooser" | "leaveDraft" | "clubDraft" | null = null;
let errorMessage = "";
let successMessage = "";
let loading = false;
let loadingMessage = "";
let arrivalValue = "";
let bonusChoice = false;
let todayPlan: TodayPlan | null = null;
let savedLoginId: string | null = null;
let draftKind: "leave" | "club" | null = null;
let draftPoll: number | null = null;
let draftNotice: { tone: "info" | "success" | "error"; text: string } | null = null;
const leaveKinds: Array<{ value: LeaveKind; label: string }> = [
  { value: "annual", label: "연차" },
  { value: "amHalf", label: "오전반차" },
  { value: "pmHalf", label: "오후반차" },
];
let leaveForm = { kind: "annual" as LeaveKind, from: "", to: "", reason: "" };
let clubForm = {
  month: "",
  clubName: "커피",
  cardName: "",
  attendees: [] as string[],
  perPerson: 30000,
  photoLink: "",
  receipts: [] as string[],
};

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
  if (error && typeof error === "object" && "message" in error && typeof error.message === "string") return error.message;
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
  if (loading) return `<div class="banner loading" role="status">${escapeHtml(loadingMessage)}</div>`;
  return "";
}

function draftBanner(): string {
  if (!draftNotice) return "";
  const role = draftNotice.tone === "error" ? "alert" : "status";
  const tone = draftNotice.tone === "info" ? "loading" : draftNotice.tone;
  return `<div class="banner ${tone} draft-banner" role="${role}"><span>${escapeHtml(draftNotice.text)}</span><button class="banner-close" id="draft-notice-close" type="button" aria-label="안내 닫기">×</button></div>`;
}

function localDate(date: Date): string {
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}-${String(date.getDate()).padStart(2, "0")}`;
}

/** 대상 월 선택지: 지난달(기본)과 이번 달. 값은 YYYY-MM 입니다. */
function clubMonthOptions(): Array<{ value: string; label: string }> {
  const now = new Date();
  const previous = new Date(now.getFullYear(), now.getMonth() - 1, 1);
  return [previous, new Date(now.getFullYear(), now.getMonth(), 1)].map((date) => ({
    value: `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}`,
    label: `${date.getFullYear()}년 ${date.getMonth() + 1}월`,
  }));
}

function formatWon(amount: number): string {
  if (amount > 0 && amount % 10000 === 0) return `${amount / 10000}만원`;
  return `${amount.toLocaleString("ko-KR")}원`;
}

/** 결재 본문에 붙여 넣을 참석자·예산 문구입니다. */
function clubPreviewText(): string {
  const month = Number(clubForm.month.split("-")[1] ?? 0);
  const count = clubForm.attendees.length;
  const lines = [
    `${month}월 참석자 : ${clubForm.attendees.join(", ")}`,
    `${count}명 * ${formatWon(clubForm.perPerson)} = ${formatWon(count * clubForm.perPerson)}`,
  ];
  const link = clubForm.photoLink.trim();
  if (link) lines.push(`활동 사진 : ${link}`);
  return lines.join("\n");
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
    <p>그룹웨어에서 이번 달 출결을 바로 불러오거나, 아마란스에서 내려받은 출결 상세 엑셀 파일을 열거나 끌어다 놓으세요.</p>
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
  return `<section class="report-section">
    <div class="report-heading">
      <div><p class="eyebrow">${escapeHtml(source === "groupware" ? "그룹웨어 (로그인한 본인 기록)" : fileName(currentPath))}</p><h2>${escapeHtml(report.period)} 근무시간</h2></div>
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
      ${renderTodayPanel(report)}
      <section class="panel notes-panel">
        <div class="panel-heading"><div><p class="eyebrow">적용된 기준</p><h3>비고</h3></div></div>
        <ul>${report.notes.map((note) => `<li>${escapeHtml(note)}</li>`).join("")}</ul>
      </section>
    </div>
  </section>`;
}

function renderTodayPanel(report: Report): string {
  const heading = `<div class="panel-heading"><div><p class="eyebrow">오늘의 계산</p><h3>오늘 퇴근 목표</h3></div></div>`;
  if (report.inProgressArrival) {
    return `<section class="panel today-panel">${heading}
      <div class="today-fields">
        <div class="today-value"><span>출근 시각 (아마란스 기록)</span><strong>${escapeHtml(report.inProgressArrival)}</strong></div>
        <label>오늘 근태<select id="today-bonus-auto">
          <option value="general" ${bonusChoice ? "" : "selected"}>일반 근무</option>
          <option value="bonus" ${bonusChoice ? "selected" : ""}>4시간 가산 (반차·생일휴가·단축·건강검진)</option>
        </select></label>
      </div>
      ${todayPlan ? renderTodayPlan(todayPlan) : `<div class="today-result" role="status"><span>계산하고 있습니다…</span></div>`}
    </section>`;
  }
  return `<section class="panel today-panel">${heading}
    <p class="today-hint">오늘 출근 기록이 없어 직접 입력합니다.</p>
    <div class="today-fields">
      <label>출근 시각<input id="today-arrival" type="time" value="${escapeHtml(arrivalValue)}" /></label>
      <label>오늘 근태<select id="today-bonus">
        <option value="general" ${bonusChoice ? "" : "selected"}>일반 근무</option>
        <option value="bonus" ${bonusChoice ? "selected" : ""}>4시간 가산 (반차·생일휴가·단축·건강검진)</option>
      </select></label>
    </div>
    <button class="secondary" id="today-calculate" type="button">계산</button>
    ${todayPlan ? renderTodayPlan(todayPlan) : ""}
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
  if (activeModal === "credentials") {
    const statusText = savedLoginId
      ? `<p class="credentials-status">현재 <strong>${escapeHtml(savedLoginId)}</strong> 계정으로 자동 로그인하도록 저장되어 있습니다.</p>`
      : `<p class="credentials-status">저장된 자동 로그인 계정이 없습니다.</p>`;
    return `<div class="modal-backdrop"><section class="modal credentials-dialog" role="dialog" aria-modal="true" aria-labelledby="modal-title">
      <h2 id="modal-title">자동 로그인 설정</h2>
      <div class="modal-content">
        ${statusText}
        <fieldset class="credentials-fields">
          <legend>그룹웨어 계정</legend>
          <label>아이디<input id="credentials-login-id" type="text" autocomplete="username" value="${escapeHtml(savedLoginId ?? "")}" /></label>
          <label>비밀번호<input id="credentials-password" type="password" autocomplete="current-password" /></label>
        </fieldset>
        <p class="credentials-hint">비밀번호는 이 기기의 키체인(Windows는 자격 증명 관리자)에만 저장되며, 앱 설정 파일에는 저장되지 않습니다.</p>
      </div>
      <div class="modal-actions">
        <button class="secondary" id="credentials-cancel" type="button">취소</button>
        <button class="delete-rule" id="credentials-delete" type="button" ${savedLoginId ? "" : "disabled"}>삭제</button>
        <button class="primary" id="credentials-save" type="button">저장</button>
      </div>
    </section></div>`;
  }
  if (activeModal === "draftChooser") {
    return `<div class="modal-backdrop"><section class="modal draft-dialog" role="dialog" aria-modal="true" aria-labelledby="modal-title">
      <h2 id="modal-title">기안 작성</h2>
      <div class="modal-content">
        <p class="draft-hint">고른 값으로 그룹웨어 양식을 미리 채워 엽니다. 상신(신청완료)은 그룹웨어 창에서 직접 누르세요.</p>
        <div class="draft-choices">
          <button class="draft-choice" id="draft-choose-leave" type="button"><strong>연차·반차</strong><span>연차휴가신청서</span></button>
          <button class="draft-choice" id="draft-choose-club" type="button"><strong>동아리 활동 내역서</strong><span>카드 내역·용도·내용 자동 입력</span></button>
        </div>
      </div>
      <div class="modal-actions"><button class="secondary" id="draft-cancel" type="button">닫기</button></div>
    </section></div>`;
  }
  if (activeModal === "leaveDraft") {
    const half = leaveForm.kind !== "annual";
    return `<div class="modal-backdrop"><section class="modal draft-dialog" role="dialog" aria-modal="true" aria-labelledby="modal-title">
      <h2 id="modal-title">연차·반차 신청</h2>
      <div class="modal-content draft-form">
        <fieldset class="draft-fieldset">
          <legend>종류</legend>
          <div class="radio-grid three">${leaveKinds.map((option) => `<label class="radio-option">
            <input type="radio" name="leave-kind" value="${option.value}" ${leaveForm.kind === option.value ? "checked" : ""} />
            <span>${escapeHtml(option.label)}</span>
          </label>`).join("")}</div>
        </fieldset>
        <div class="draft-row">
          <label>${half ? "날짜" : "시작일"}<input id="leave-from" type="date" value="${escapeHtml(leaveForm.from)}" /></label>
          ${half ? "" : `<label>종료일<input id="leave-to" type="date" value="${escapeHtml(leaveForm.to)}" /></label>`}
        </div>
        <label>사유 (선택)<input id="leave-reason" type="text" value="${escapeHtml(leaveForm.reason)}" placeholder="비고(사유)에 들어갑니다" /></label>
      </div>
      <div class="modal-actions"><button class="secondary" id="draft-back" type="button">뒤로</button><button class="primary" id="leave-open" type="button">양식 열기</button></div>
    </section></div>`;
  }
  if (activeModal === "clubDraft") {
    const attendees = clubForm.attendees.length === 0
      ? `<p class="empty-rules">참석자를 추가하세요</p>`
      : `<ul class="attendee-list">${clubForm.attendees.map((name, index) => `<li><span>${escapeHtml(name)}</span><button class="delete-rule" data-remove-attendee="${index}" type="button">삭제</button></li>`).join("")}</ul>`;
    return `<div class="modal-backdrop"><section class="modal draft-dialog" role="dialog" aria-modal="true" aria-labelledby="modal-title">
      <h2 id="modal-title">동아리 활동 내역서</h2>
      <div class="modal-content draft-form">
        <div class="draft-row">
          <label>대상 월<select id="club-month">${clubMonthOptions().map((option) => `<option value="${option.value}" ${clubForm.month === option.value ? "selected" : ""}>${escapeHtml(option.label)}</option>`).join("")}</select></label>
          <label>동아리명<input id="club-name" type="text" value="${escapeHtml(clubForm.clubName)}" /></label>
        </div>
        <div class="draft-row">
          <label>카드 이름 (목록에서 이 글자가 들어간 카드)<input id="club-card" type="text" value="${escapeHtml(clubForm.cardName)}" placeholder="예: 동아리_커피" /></label>
          <label>1인당 금액(원)<input id="club-per-person" type="number" min="0" step="1000" value="${clubForm.perPerson}" /></label>
        </div>
        <fieldset class="draft-fieldset">
          <legend>참석자 (${clubForm.attendees.length}명)</legend>
          ${attendees}
          <div class="attendee-add"><input id="club-attendee-input" type="text" placeholder="이름 입력 후 Enter" /><button class="secondary" id="club-attendee-add" type="button">추가</button></div>
        </fieldset>
        <fieldset class="draft-fieldset">
          <legend>영수증 파일 (${clubForm.receipts.length}개, 선택)</legend>
          ${clubForm.receipts.length === 0 ? "" : `<ul class="receipt-list">${clubForm.receipts.map((path, index) => `<li><span>${escapeHtml(fileName(path))}</span><button class="delete-rule" data-remove-receipt="${index}" type="button">삭제</button></li>`).join("")}</ul>`}
          <button class="secondary" id="club-receipts-pick" type="button">영수증 파일 선택</button>
          <p class="draft-hint receipt-hint">jpg·png·gif·heic·pdf, 파일당 20MB·합계 50MB까지. 그룹웨어 창에서 결재상신을 누르면 뜨는 결재 작성 창에 앱이 첨부합니다(저장하지 않음).</p>
        </fieldset>
        <label>활동 사진 링크 (선택, 저장하지 않음)<input id="club-photo-link" type="url" value="${escapeHtml(clubForm.photoLink)}" placeholder="Slack 메시지 링크 등" /></label>
        <div class="club-preview">
          <div class="club-preview-heading"><span>결재 본문에 붙여 넣을 문구</span><button class="secondary" id="club-copy" type="button">복사</button></div>
          <pre id="club-preview-text">${escapeHtml(clubPreviewText())}</pre>
        </div>
        <p class="draft-hint">제목 입력 → 카드사용내역에서 대상 월 카드 내역 전체 선택·확인 → 행마다 용도(복리후생비_동아리)·내용을 채웁니다. 카드 영수증 이미지는 카드 내역 확인 시 그룹웨어가 붙이고, 위에서 고른 영수증 파일은 결재 작성 창에 앱이 첨부합니다. 첨부 목록을 확인하세요. 활동 사진은 Slack 링크를 위 칸에 넣고 복사한 문구를 본문에 붙여 넣으세요.</p>
      </div>
      <div class="modal-actions"><button class="secondary" id="draft-back" type="button">뒤로</button><button class="primary" id="club-open" type="button">양식 열기</button></div>
    </section></div>`;
  }
  return "";
}

function render(): void {
  app.innerHTML = `<div class="app-shell">
    <header class="topbar">
      <div class="brand"><span class="brand-mark">A</span><div><h1>근무시간 계산기</h1><p>아마란스 출결 월간 정리</p></div></div>
      <nav class="header-actions" aria-label="파일 및 규칙">
        <button class="primary" id="groupware-button" type="button">그룹웨어에서 불러오기</button>
        <button class="secondary" id="draft-button" type="button">기안 작성</button>
        <button class="secondary" id="credentials-button" type="button">자동 로그인 설정</button>
        <button class="secondary" id="open-button" type="button">엑셀 파일 열기</button>
        <button class="secondary" id="rules-button" type="button">규칙 설정</button>
      </nav>
    </header>
    ${banner()}
    ${draftBanner()}
    ${renderDropZone()}
    ${report ? renderReport(report) : renderEmpty()}
    ${renderModal()}
    <footer>계산은 이 기기에서만 처리됩니다. 그룹웨어 로그인 세션은 이 기기의 앱 설정 폴더에만 저장됩니다.</footer>
  </div>`;
  bindControls();
}

function stopLoginPoll(): void {
  if (loginPoll !== null) window.clearInterval(loginPoll);
  loginPoll = null;
}

function setError(error: unknown): void {
  stopLoginPoll();
  errorMessage = errorText(error);
  successMessage = "";
  loading = false;
  render();
}

function applyAnalysis(analysis: Analysis): void {
  loading = false;
  if (analysis.status === "needsRules") {
    unknownItems = analysis.unknownItems;
    activeModal = "unknown";
    render();
    return;
  }
  report = analysis.report;
  arrivalValue = report.inProgressArrival ?? "";
  bonusChoice = report.inProgressBonus;
  render();
  if (report.inProgressArrival) void autoPlanToday(report);
}

async function autoPlanToday(current: Report): Promise<void> {
  try {
    const plan = await invoke<TodayPlan>("plan_today", {
      summary: current.summary,
      arrival: current.inProgressArrival,
      bonus: bonusChoice,
    });
    if (report !== current) return;
    todayPlan = plan;
    render();
  } catch (error) {
    setError(error);
  }
}

function resetResult(message: string): void {
  report = null;
  unknownItems = [];
  activeModal = null;
  todayPlan = null;
  arrivalValue = "";
  errorMessage = "";
  successMessage = "";
  loading = true;
  loadingMessage = message;
  render();
}

async function loadGroupware(): Promise<void> {
  stopLoginPoll();
  source = "groupware";
  currentPath = null;
  resetResult("그룹웨어에서 이번 달 출결을 불러오고 있습니다…");
  await fetchGroupware();
}

async function fetchGroupware(): Promise<void> {
  if (fetchInFlight) return;
  fetchInFlight = true;
  try {
    await fetchGroupwareOnce();
  } finally {
    fetchInFlight = false;
  }
}

async function fetchGroupwareOnce(): Promise<void> {
  try {
    const analysis = await invoke<Analysis>("fetch_groupware");
    stopLoginPoll();
    applyAnalysis(analysis);
  } catch (raw) {
    const error = raw as GroupwareError;
    if (error?.code === "failed" && loginPoll !== null && ++loginFailures < 10) return;
    if (error?.code === "loginRequired") {
      if (loginPoll !== null) {
        if (!error.windowOpen) setError("로그인 창이 닫혔습니다. 다시 불러오려면 '그룹웨어에서 불러오기'를 누르세요.");
        return;
      }
      let auto = false;
      try {
        auto = await invoke<boolean>("open_groupware_login");
      } catch (openError) {
        setError(openError);
        return;
      }
      loading = true;
      loadingMessage = auto
        ? "저장된 계정으로 로그인하는 중…"
        : "그룹웨어 로그인 창에서 로그인하면 자동으로 불러옵니다… (자동 로그인은 '자동 로그인 설정'에서 설정할 수 있습니다)";
      render();
      loginFailures = 0;
      loginPoll = window.setInterval(() => void fetchGroupware(), 2000);
      return;
    }
    setError(raw);
  }
}

async function reanalyze(): Promise<void> {
  if (source === "groupware") await loadGroupware();
  else if (currentPath) await analyzeCurrent(currentPath);
}

async function analyzeCurrent(path: string): Promise<void> {
  stopLoginPoll();
  source = "file";
  currentPath = path;
  report = null;
  unknownItems = [];
  activeModal = null;
  todayPlan = null;
  arrivalValue = "";
  errorMessage = "";
  successMessage = "";
  loading = true;
  loadingMessage = "엑셀 파일을 분석하고 있습니다…";
  render();
  try {
    applyAnalysis(await invoke<Analysis>("analyze_file", { path }));
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
  if (!report || (source === "file" && !currentPath)) return;
  errorMessage = "";
  successMessage = "";
  try {
    const output = await save({
      defaultPath: `근무시간_${report.period}.xlsx`,
      filters: [{ name: "Excel", extensions: ["xlsx"] }],
    });
    if (!output) return;
    if (source === "groupware") await invoke("export_groupware_report", { outPath: output });
    else await invoke("export_report", { path: currentPath, outPath: output });
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
    await reanalyze();
  } catch (error) {
    setError(error);
  }
}

async function showCredentials(): Promise<void> {
  errorMessage = "";
  successMessage = "";
  try {
    savedLoginId = await invoke<string | null>("groupware_credentials");
    activeModal = "credentials";
    render();
  } catch (error) {
    setError(error);
  }
}

async function saveCredentials(): Promise<void> {
  const loginId = document.querySelector<HTMLInputElement>("#credentials-login-id")?.value.trim() ?? "";
  const password = document.querySelector<HTMLInputElement>("#credentials-password")?.value ?? "";
  if (!loginId || !password) {
    errorMessage = "아이디와 비밀번호를 모두 입력하세요.";
    render();
    return;
  }
  try {
    await invoke("save_groupware_credentials", { loginId, password });
    activeModal = null;
    errorMessage = "";
    successMessage = "자동 로그인 계정을 저장했습니다.";
    render();
  } catch (error) {
    setError(error);
  }
}

async function clearCredentials(): Promise<void> {
  try {
    await invoke("clear_groupware_credentials");
    activeModal = null;
    errorMessage = "";
    successMessage = "자동 로그인 계정을 삭제했습니다.";
    render();
  } catch (error) {
    setError(error);
  }
}

async function deleteRule(item: string): Promise<void> {
  try {
    await invoke("delete_item_rule", { item });
    if (source) {
      activeModal = null;
      await reanalyze();
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

function stopDraftPoll(): void {
  if (draftPoll !== null) window.clearInterval(draftPoll);
  draftPoll = null;
}

async function showDraftChooser(): Promise<void> {
  try {
    const settings = await invoke<DraftSettings>("draft_settings");
    const today = localDate(new Date());
    leaveForm = { kind: leaveForm.kind, from: leaveForm.from || today, to: leaveForm.to || today, reason: settings.leaveReason };
    clubForm = {
      month: clubForm.month || clubMonthOptions()[0].value,
      clubName: settings.clubName,
      cardName: settings.cardName,
      attendees: [...settings.attendees],
      perPerson: settings.perPerson,
      photoLink: clubForm.photoLink,
      receipts: clubForm.receipts,
    };
    activeModal = "draftChooser";
    render();
  } catch (error) {
    setError(error);
  }
}

function readClubFields(): void {
  clubForm.month = document.querySelector<HTMLSelectElement>("#club-month")?.value ?? clubForm.month;
  clubForm.clubName = document.querySelector<HTMLInputElement>("#club-name")?.value ?? clubForm.clubName;
  clubForm.cardName = document.querySelector<HTMLInputElement>("#club-card")?.value ?? clubForm.cardName;
  const perPerson = Number(document.querySelector<HTMLInputElement>("#club-per-person")?.value ?? clubForm.perPerson);
  clubForm.perPerson = Number.isFinite(perPerson) && perPerson >= 0 ? Math.round(perPerson) : 0;
  clubForm.photoLink = document.querySelector<HTMLInputElement>("#club-photo-link")?.value ?? clubForm.photoLink;
}

function updateClubPreview(): void {
  readClubFields();
  const preview = document.querySelector<HTMLElement>("#club-preview-text");
  if (preview) preview.textContent = clubPreviewText();
}

function addAttendee(): void {
  readClubFields();
  const input = document.querySelector<HTMLInputElement>("#club-attendee-input");
  const names = (input?.value ?? "").split(/[,，]/).map((name) => name.trim()).filter(Boolean);
  clubForm.attendees.push(...names.filter((name) => !clubForm.attendees.includes(name)));
  render();
  document.querySelector<HTMLInputElement>("#club-attendee-input")?.focus();
}

async function copyClubPreview(): Promise<void> {
  readClubFields();
  const text = clubPreviewText();
  try {
    await navigator.clipboard.writeText(text);
  } catch {
    const area = document.createElement("textarea");
    area.value = text;
    document.body.appendChild(area);
    area.select();
    document.execCommand("copy");
    area.remove();
  }
  const button = document.querySelector<HTMLButtonElement>("#club-copy");
  if (button) button.textContent = "복사됨";
}

async function pickReceipts(): Promise<void> {
  readClubFields();
  try {
    const selected = await open({
      multiple: true,
      filters: [{ name: "영수증", extensions: ["jpg", "jpeg", "png", "gif", "heic", "pdf"] }],
    });
    const paths = Array.isArray(selected) ? selected : typeof selected === "string" ? [selected] : [];
    clubForm.receipts.push(...paths.filter((path) => !clubForm.receipts.includes(path)));
    render();
  } catch (error) {
    draftNotice = { tone: "error", text: errorText(error) };
    render();
  }
}

function draftDoneText(): string {
  return draftKind === "club"
    ? "그룹웨어 창에서 내용을 확인한 뒤 상신하세요. 카드 영수증 이미지는 카드 내역 확인 시 그룹웨어가 붙입니다. 첨부 목록을 확인하세요. 활동 사진은 복사한 문구로 본문에 링크를 붙여 넣으세요."
    : "그룹웨어 창에서 내용을 확인한 뒤 신청완료를 직접 누르세요.";
}

async function pollDraftStatus(): Promise<void> {
  try {
    const status = await invoke<DraftStatus>("draft_status");
    switch (status.state) {
      case "opening":
        draftNotice = { tone: "info", text: "그룹웨어 기안 창을 여는 중…" };
        break;
      case "login":
        draftNotice = { tone: "info", text: status.message || "그룹웨어 로그인이 필요합니다" };
        break;
      case "filling":
        draftNotice = { tone: "info", text: `양식을 채우는 중…${status.step ? ` (${status.step})` : ""}` };
        break;
      case "awaitingReceipts":
        draftNotice = { tone: "info", text: status.message };
        break;
      case "done":
        stopDraftPoll();
        draftNotice = { tone: "success", text: status.message || draftDoneText() };
        break;
      case "empty":
        stopDraftPoll();
        draftNotice = { tone: "error", text: `${status.message || "반영할 카드 내역이 없습니다"}. 카드사용 내역 창을 열어 두었으니 기간·카드를 확인하세요.` };
        break;
      case "failed":
        stopDraftPoll();
        draftNotice = { tone: "error", text: `기안 채우기를 멈췄습니다${status.step ? ` — ${status.step}` : ""}: ${status.message}. 그룹웨어 창은 열어 두었습니다.` };
        break;
      case "closed":
        stopDraftPoll();
        draftNotice = { tone: "info", text: "기안 창이 닫혔습니다." };
        break;
      default:
        return;
    }
    render();
  } catch (error) {
    stopDraftPoll();
    draftNotice = { tone: "error", text: errorText(error) };
    render();
  }
}

async function openDraft(kind: "leave" | "club", command: string, request: unknown): Promise<void> {
  stopDraftPoll();
  try {
    await invoke(command, { request });
    draftKind = kind;
    activeModal = null;
    errorMessage = "";
    draftNotice = { tone: "info", text: "그룹웨어 기안 창을 여는 중…" };
    render();
    draftPoll = window.setInterval(() => void pollDraftStatus(), 1000);
  } catch (error) {
    draftNotice = { tone: "error", text: errorText(error) };
    render();
  }
}

async function openLeaveDraft(): Promise<void> {
  leaveForm.from = document.querySelector<HTMLInputElement>("#leave-from")?.value ?? "";
  leaveForm.to = leaveForm.kind === "annual" ? document.querySelector<HTMLInputElement>("#leave-to")?.value ?? "" : leaveForm.from;
  leaveForm.reason = document.querySelector<HTMLInputElement>("#leave-reason")?.value ?? "";
  if (!leaveForm.from || !leaveForm.to) {
    draftNotice = { tone: "error", text: "날짜를 입력하세요." };
    render();
    return;
  }
  if (leaveForm.to < leaveForm.from) {
    draftNotice = { tone: "error", text: "종료일이 시작일보다 빠릅니다." };
    render();
    return;
  }
  await openDraft("leave", "open_leave_draft", { ...leaveForm });
}

async function openClubDraft(): Promise<void> {
  readClubFields();
  const [year, month] = clubForm.month.split("-").map(Number);
  if (!clubForm.clubName.trim() || !clubForm.cardName.trim()) {
    draftNotice = { tone: "error", text: "동아리명과 카드 이름을 입력하세요." };
    render();
    return;
  }
  await openDraft("club", "open_club_draft", {
    year,
    month,
    clubName: clubForm.clubName,
    cardName: clubForm.cardName,
    attendees: clubForm.attendees,
    perPerson: clubForm.perPerson,
    receipts: clubForm.receipts,
  });
}

function bindControls(): void {
  document.querySelector("#open-button")?.addEventListener("click", () => void chooseFile());
  document.querySelector("#groupware-button")?.addEventListener("click", () => void loadGroupware());
  document.querySelector("#credentials-button")?.addEventListener("click", () => void showCredentials());
  document.querySelector("#rules-button")?.addEventListener("click", () => void showRules());
  document.querySelector("#save-button")?.addEventListener("click", () => void saveReport());
  document.querySelector("#today-calculate")?.addEventListener("click", () => void calculateToday());
  document.querySelector<HTMLSelectElement>("#today-bonus-auto")?.addEventListener("change", (event) => {
    bonusChoice = (event.target as HTMLSelectElement).value === "bonus";
    if (report) void autoPlanToday(report);
  });
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
  document.querySelector("#credentials-cancel")?.addEventListener("click", () => {
    activeModal = null;
    render();
  });
  document.querySelector("#credentials-save")?.addEventListener("click", () => void saveCredentials());
  document.querySelector("#credentials-delete")?.addEventListener("click", () => void clearCredentials());
  document.querySelectorAll<HTMLButtonElement>("[data-delete-rule]").forEach((button) => {
    button.addEventListener("click", () => void deleteRule(button.dataset.deleteRule ?? ""));
  });
  bindDraftControls();
}

function bindDraftControls(): void {
  document.querySelector("#draft-button")?.addEventListener("click", () => void showDraftChooser());
  document.querySelector("#draft-notice-close")?.addEventListener("click", () => {
    draftNotice = null;
    render();
  });
  document.querySelector("#draft-cancel")?.addEventListener("click", () => {
    activeModal = null;
    render();
  });
  document.querySelector("#draft-back")?.addEventListener("click", () => {
    if (activeModal === "clubDraft") readClubFields();
    activeModal = "draftChooser";
    render();
  });
  document.querySelector("#draft-choose-leave")?.addEventListener("click", () => {
    activeModal = "leaveDraft";
    render();
  });
  document.querySelector("#draft-choose-club")?.addEventListener("click", () => {
    activeModal = "clubDraft";
    render();
  });
  document.querySelectorAll<HTMLInputElement>('input[name="leave-kind"]').forEach((input) => {
    input.addEventListener("change", () => {
      leaveForm.from = document.querySelector<HTMLInputElement>("#leave-from")?.value ?? leaveForm.from;
      leaveForm.to = document.querySelector<HTMLInputElement>("#leave-to")?.value ?? leaveForm.to;
      leaveForm.reason = document.querySelector<HTMLInputElement>("#leave-reason")?.value ?? leaveForm.reason;
      leaveForm.kind = input.value as LeaveKind;
      render();
    });
  });
  document.querySelector("#leave-open")?.addEventListener("click", () => void openLeaveDraft());
  ["#club-month", "#club-name", "#club-card", "#club-per-person", "#club-photo-link"].forEach((selector) => {
    document.querySelector(selector)?.addEventListener("input", updateClubPreview);
  });
  document.querySelector("#club-attendee-add")?.addEventListener("click", addAttendee);
  document.querySelector<HTMLInputElement>("#club-attendee-input")?.addEventListener("keydown", (event) => {
    if (event.key === "Enter" && !event.isComposing) {
      event.preventDefault();
      addAttendee();
    }
  });
  document.querySelectorAll<HTMLButtonElement>("[data-remove-attendee]").forEach((button) => {
    button.addEventListener("click", () => {
      readClubFields();
      clubForm.attendees.splice(Number(button.dataset.removeAttendee), 1);
      render();
    });
  });
  document.querySelector("#club-copy")?.addEventListener("click", () => void copyClubPreview());
  document.querySelector("#club-receipts-pick")?.addEventListener("click", () => void pickReceipts());
  document.querySelectorAll<HTMLButtonElement>("[data-remove-receipt]").forEach((button) => {
    button.addEventListener("click", () => {
      readClubFields();
      clubForm.receipts.splice(Number(button.dataset.removeReceipt), 1);
      render();
    });
  });
  document.querySelector("#club-open")?.addEventListener("click", () => void openClubDraft());
}

render();
void loadGroupware();
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
