# 그룹웨어 결재 양식 자동 입력 조사 (Amaranth 10, gwa.soomgo.com)

조사일 2026-11-20. 읽기 전용으로 조사했다(상신·저장·임시저장 없음). 화면은 모두 React 16 계열(`__reactInternalInstance$…`)이고, 더존 OBT 컴포넌트(`OBTDatePickerRebuild`, `OBTButton`, `OBTDataGrid`=RealGridJS 1.1.36)를 쓴다.

## 공통 사항

- **두 양식 모두 같은 페이지 안에서 열린다**(hash 라우트 변경). 양식 진입 시 `window.open` 호출 없음(확인됨, `window.open` 후킹으로 확인).
- **직접 URL 진입 가능**(확인됨). 단, 이미 탭이 열린 상태에서 hash 만 바꾸면 현재 활성 탭으로 되돌아간다. 새로 로드(about:blank → URL)하면 해당 양식이 바로 열린다.
- 클래스 이름에 빌드 해시(`__ccItF`, `__k2YuT`)가 붙어 있으니 `[class*="OBTDatePickerRebuild_inputYMD"]` 처럼 부분 일치로 찾거나, 표의 `th` 글자(예 `휴가기간`)로 행을 찾는 방식이 안전하다.
- **OBT 날짜 입력은 네이티브 setter + `input`/`change` 이벤트로는 반영되지 않는다**(확인됨: blur 시 원래 값으로 되돌아감). 대신 React fiber 를 따라 올라가 `value:{from,to}` 를 가진 컴포넌트의 `props.onChange({value:{from:'YYYYMMDD',to:'YYYYMMDD'}})` 를 부르면 반영된다(확인됨, 재계산 API 호출까지 발생).
- 일반 텍스트 입력(OBTTextField)은 네이티브 setter + `input` 이벤트(bubbles)로 React 상태까지 반영된다(확인됨).
- 근태신청서 화면에 처음 들어가면 「공지사항」 모달이 뜬다. X 로 닫아야 조작 가능. 닫을 때 `/system/common/iniWrite`(`NOTICE_HPD0110_221`) 로 공지 읽음 표시가 저장된다(문서 저장은 아님).

---

## 양식 1 — 연차휴가신청서

### URL

```
https://gwa.soomgo.com/#/HP/HPD0110/HPD0110?MicroModuleCode=eap&docWidth=1035&formDTp=HP_HPD0110_00011&formId=36
```

- 메뉴: 임직원업무관리 > 근태관리 > 근태신청 > 근태신청서(HPD0110). 왼쪽 「근태신청서 양식 선택」 목록에서 연차휴가신청서가 선택된 상태로 열린다.
- 연차휴가신청서의 근태 연동코드 `linkAtCd=1010`, 연동 프로세스 `HP_HPD0110_00011`, 결재 양식 `formId=36`.

### 필드

| 필드 | 선택자 | 입력 방식 | 프로그램 입력 가능 여부 |
|---|---|---|---|
| 휴가구분 | `th` 글자 `휴가구분` 행의 `button`(글자 `연차`/`오전반차`/`오후반차`, `OBTButtonGroup_button`) | 버튼 그룹. 선택된 버튼에 `OBTButtonGroup_focus__…` 클래스. 코드: 연차 `1101`, 오전반차 `1102`, 오후반차 `1103` | 확인됨 — `button.click()` 로 바뀌고 시간·일수 재계산 API 호출됨. `<form>` 밖이라 `type=submit` 이어도 제출 안 됨 |
| 휴가기간(시작~종료) | `th` `휴가기간` 행의 `input[class*="OBTDatePickerRebuild_inputYMD"]` 2개 | OBT 기간 날짜 선택기. 화면 형식 `YYYY-MM-DD`, 내부 값 `{from:'YYYYMMDD', to:'YYYYMMDD'}` | 확인됨 — fiber 의 `onChange({value:{from,to}})` 만 동작. 네이티브 setter 는 안 됨 |
| 신청일수 | 같은 행 비활성 input | 자동 계산(연차 1, 반차 0.5, 2일이면 2) | 읽기 전용(자동) |
| 시작/종료시간 | `th` `시작/종료시간` 행 `OBTTimePicker2` | 반차일 때만 표시·활성. 오전반차 09:00~13:00, 오후반차는 근무 설정에 따라 자동 | 자동 채움 확인됨(수정은 미확인) |
| 신청시간 | 비활성 input | 자동(연차 08시간 00분, 반차 04시간 00분) | 읽기 전용(자동) |
| 대상자 | `input[placeholder="사원코드도움"]` | 코드도움. 기본값 본인(사번. 이름) | 기본값 그대로 쓰면 됨 |
| 비고(사유) | `th` `비고(사유)` 행 `input` | 일반 텍스트, 기본값 빈칸 | 확인됨 — 네이티브 setter + `input` 이벤트로 React 상태 반영 |

- 하단 버튼: `추가신청`, `신청완료`. **신청완료는 누르지 않았다.**
- 기본 상태: 휴가구분 강조 없음(값은 연차 기준 1일·8시간), 기간 = 오늘~오늘, 사유 빈칸.

### 결재선·두 번째 화면 — 미확인

- `신청완료` 의 핸들러는 `handleChangeStateOfClickAction("step1CompleteButton")` 뿐이라 이후 흐름은 코드로 끝까지 따라가지 못했다(미확인).
- eap 번들에는 결재 작성 화면을 `windowOpen("UBAP001", {formId, MicroModuleCode:"eap"}, "width=…,height=…")` → `window.open("/#/popup?callComp=UBAP001…")` 로 **새 창**으로 여는 코드가 있다. 연차 신청완료 뒤 결재 작성 창도 이 팝업일 가능성이 높다(미확인). Tauri 웹뷰는 `window.open` 처리(새 창 허용 또는 같은 창 이동)가 필요할 수 있다.
- 결재선 기본값은 결재 작성 화면에서만 보이므로 미확인.

### 관찰한 API(읽기용)

- `POST /human/common/attendapplication/getApplicationBaseInfo` — 근태 코드 목록(`attendCodeList`: atCd·atNm·linkAtCd·outProcessId)
- `POST /human/common/attendapplication/getStartTmEndTmByWorkType` — 휴가구분·날짜별 시작/종료시간
- `POST /human/common/attendapplication/calculateApplicationDays` — 신청일수 계산
- `POST /human/common/annualleave/getAnnualLeaveInfoOfEmployee` — 잔여 연차
- `POST /human/attendapplication/getHeaderStats`, `/human/attendapplication/at00001` — 상단 현황·신청 내역

### 권장 자동화 순서

1. 웹뷰를 위 URL 로 새로 로드 → `th` `휴가구분` 이 나타날 때까지 대기.
2. 공지사항 모달이 있으면 X 클릭.
3. 휴가구분 버튼 `.click()` (연차/오전반차/오후반차).
4. 휴가기간: fiber `onChange({value:{from,to}})` (아래 코드). 휴가구분을 먼저 고르고 날짜를 넣는다(구분 변경 시 시간 재계산).
5. 사유 입력(선택).
6. `신청완료` 는 사용자가 직접 누르게 둔다.

```js
// 휴가기간 설정 (YYYYMMDD)
function setLeavePeriod(from, to) {
  const th = [...document.querySelectorAll('th')].find(t => t.textContent.trim() === '휴가기간');
  const el = th.closest('tr').querySelector('input');
  const key = Object.keys(el).find(k => k.startsWith('__reactInternalInstance') || k.startsWith('__reactFiber'));
  for (let f = el[key]; f; f = f.return) {
    const p = f.memoizedProps;
    if (p && p.value && p.value.from !== undefined && p.onChange) { p.onChange({ value: { from, to } }); return true; }
  }
  return false;
}
// 텍스트 입력
function setText(input, v) {
  Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(input, v);
  input.dispatchEvent(new Event('input', { bubbles: true }));
}
```

---

## 양식 2 — 동아리 활동 내역서 (지출결의서 계열)

### URL

```
https://gwa.soomgo.com/#/HP/APB1020/APB1020?MicroModuleCode=eap&docWidth=1035&formDTp=APB1020_00001&formId=201
```

- 결재작성 > 인사관련문서 > 동아리 활동 내역서 를 누르면 임직원업무관리 > 지출결의서작성(APB1020) 화면으로 이동하고 양식목록에서 「동아리 활동 내역서」가 선택된다(`rowForm.formId=201`).
- 양식을 연 것만으로는 임시 문서가 생기지 않는다(확인됨: 저장 API 호출 없음, 페이지 상태 `imsiDoc.docId` 빈 값).

### 지출정보 필드

| 필드 | 선택자 | 입력 방식 | 프로그램 입력 가능 여부 |
|---|---|---|---|
| 회계처리일자 | `th` 글자가 `회계처리일자` 로 끝나는 행의 `input[class*="OBTDatePickerRebuild_inputYMD"]` | OBT 단일 날짜 선택기, 기본값 오늘(`YYYY-MM-DD`) | 기본값이 오늘이라 입력 불필요. 바꿔야 하면 fiber onChange 방식(미확인) |
| 제목 | `th` `제목` 다음 칸의 `input` | 텍스트(필수, 분홍 배경) | 확인됨 — 네이티브 setter + `input` 이벤트로 `state.expend.head.docTitle` 반영 |
| 회계단위 | `input[placeholder="사업장코드"]` | 기본 `1000. (주)브레이브모바일` | 기본값 사용 |
| 전표처리구분 | radio `Y`(지출결의서별)/`N`(지출내역별, 기본) | 라디오 | 건드릴 필요 없음 |
| 자금집행전용 | radio `1`(부, 기본)/`0`(여) | 라디오 | 건드릴 필요 없음 |
| 카드사용내역 버튼 | 글자 `카드사용내역` 인 `button` | 페이지 안 모달(`.dialog_data`) 열기 | 확인됨 — `.click()` 으로 열림, `window.open` 없음 |

### 카드사용내역 팝업 구조

| 요소 | 선택자/접근 | 비고 | 프로그램 입력 가능 여부 |
|---|---|---|---|
| 팝업 루트 | `.dialog_data` | 제목 「카드사용 내역」 | — |
| 지출일자 기간 | `.dialog_data input[class*="OBTDatePickerRebuild_inputYMD"]` 2개 | 기본값 이번 달 1일~오늘 | 확인됨 — fiber onChange `{value:{from,to}}` 로 변경 |
| 조회 | `.dialog_data` 안 글자 `조회` 버튼 | 기간 바꾼 뒤 눌러야 목록 갱신 | 확인됨 — `.click()` |
| 탭 | `미반영`(기본) / `반영완료` | 미반영만 가져올 수 있음 | — |
| 카드 목록(왼쪽) | `.dialog_data li.type1` (카드 이름 글자로 찾음) | 첫 항목 「전체」, 그 아래 카드별 | 확인됨 — `li.click()` 으로 선택, 오른쪽 그리드 갱신 |
| 사용 내역 그리드(오른쪽) | canvas(RealGrid). 팝업 컴포넌트 `state.cardDataGridTab1`(OBTDataGrid 인터페이스) | 열: 지출일자·거래처·사업자번호·공급가액·부가세·금액·승인번호 … | 확인됨 — `grid.checkAll(true)` 로 전체 체크(체크 수 = 행 수). 헤더 체크박스는 canvas 라 DOM 클릭 대상 아님 |
| 확인 / 취소 | 글자 `확인` / `취소` 버튼 | | **확인은 누르지 않았다**(아래 차단 요소 참고) |

이 사용자에게 보이는 카드:

- 카드 5장(업무용·실활동비·해외결제·동아리 카드). 이름과 번호는 적지 않는다.
- 동아리 카드 이름에는 `동아리` 와 동아리명이 들어간다. 앱은 설정한 「카드 이름」 글자가 들어간 항목을 고른다.

2026-01-01~2026-11-20 미반영 탭 기준 동아리 카드 사용 내역은 0건이라, 동아리 카드로 행 선택·반영은 시험하지 못했다(다른 카드로 선택·전체 체크까지만 확인).

### 지출내역 그리드

- **canvas 기반 RealGridJS**(`window.RealGridJS`, `window.Grids` 존재). DOM 셀이 없어 셀 클릭·타이핑으로 넣기 어렵다.
- canvas 의 부모 div 에서 fiber 를 올라가면 `memoizedProps.interface` 가 OBTDataGrid 인터페이스다(`getRowCount`, `getRow`, `getValue`, `setValue`, `getColumns`, `checkAll`, `getCheckedIndexes` 등). 같은 객체를 페이지 컴포넌트 `state.component.grid` 로도 얻을 수 있다.
- 페이지 컴포넌트: fiber 를 올라가 `stateNode.handle.change` 가 있는 것. `state.expend.head`(제목·회계처리일자 등), `state.expend.detail`(지출내역 행) 을 가진다.
- 주요 열: `cashNm`(용도, codePicker) + 숨은 `cashCd`, `rmkDc`(내용, text), `supAm`/`vatAm`/`sumAm`, `attrNm`(증빙) …
- **용도는 코드도움(codePicker)** — `ACASHCD_TYPE_CODE`. 이 양식(formId 201)에 연결된 용도는 하나뿐: `typeCd 2204 / 복리후생비_동아리`(목록 API `POST /personal/APCodePicker/ApAcashcdTypeCode` 응답). 선택 시 코드도움 콜백이 `handle.change.cashType(rowIndex, item)` 를 불러 `cashNm`·`cashCd`·각종 플래그를 넣는다.

| 필드 | 접근 | 입력 방식 | 프로그램 입력 가능 여부 |
|---|---|---|---|
| 용도 | `page.handle.change.cashType(row, {typeCd:'2204', typeNm:'복리후생비_동아리', deductFg:'', taxFg:'', nonsubTy:'', deptCheckYn:'N', pjtCheckYn:'N', budgetCheckYn:'N', trcdCheckYn:'N', accountCheckYn:'N', carYn:'N'})` | codePicker(팝업 검색) 대신 콜백 직접 호출 | 확인됨 — 그리드와 `state.expend.detail[row]` 모두 `cashNm=복리후생비_동아리`, `cashCd=2204` 로 반영 |
| 내용 | `grid.setValue(row, 'rmkDc', '동아리명_동아리활동')` | 텍스트 셀 | 확인됨 — 그리드·상태 모두 반영 |

시험 값은 모두 빈 값으로 되돌렸다. 기본 빈 행 1개 상태에서 시험했으므로, 카드 내역을 실제로 가져온 행에서의 동작(검증결과 「적합」 전환 등)은 미확인.

### 차단 요소

1. **카드사용내역 팝업의 「확인」이 서버 쓰기를 동반할 수 있다.** `cardConfirm` 은 선택 행을 지출내역에 넣은 뒤 `createImgFg=7` 설정 때문에 영수증 이미지 생성 과정(`handle.img.startImgProcess`)을 돈다. 이 과정은 `POST /personal/APB1020/0ap01004`(로그 기록)을 부르고 html2canvas 로 이미지를 만들어 첨부로 올리는 것으로 보인다. 문서 저장은 아니지만 서버 쓰기라 시험하지 않았다(미확인). 자동화에서는 「확인」까지는 사용자가 누르게 하거나, 사용자 동의 후 실행하는 편이 안전하다.
2. 카드 조회 API 요청 바디 일부가 암호화(`@!E!@…@!E!@`)되어 있어 API 를 직접 부르는 방식은 어렵다. UI(fiber) 조작이 현실적이다.
3. 동아리 카드 미반영 내역이 0건이라 실제 동아리 행 흐름은 미확인.
4. 화면 폭이 좁으면 지출내역 그리드 영역이 접혀 보이지 않는다(데이터 조작에는 영향 없음).

### 결재상신 후 두 번째 화면 — 미확인

- `결재상신` 은 페이지 공통 기능 버튼(`handleFunctionButtonClick`)이다. 결재 쪽 콜백 `handle.approval.approvalListDataMake({linkKey, approKey, menuCode})` 가 지출 데이터를 `api.setExpend` 로 **저장**한다. 즉 결재 작성 화면(eap)이 열리고 연동 키가 돌아온 시점에 지출결의 데이터가 저장되는 구조로 보인다(미확인).
- eap 번들 코드상 결재 작성 화면은 `/#/popup?callComp=UBAP001&formId=…&MicroModuleCode=eap` 를 `window.open` 으로 띄우는 **새 창**일 가능성이 높다(미확인). 파란 영역(참석자·인원·예산)은 그 창의 본문 편집기(HTML 양식) 안에 있을 것으로 보이며, 선택자·입력 방식은 미확인.

### 권장 자동화 순서

1. 웹뷰를 위 URL 로 새로 로드 → `카드사용내역` 버튼이 나타날 때까지 대기.
2. 제목: 네이티브 setter + `input` 이벤트로 `[동아리] [동아리명] ○월 활동 내역서`.
3. 회계처리일자: 기본값(오늘) 유지.
4. `카드사용내역` 클릭 → `.dialog_data` 대기 → 기간 fiber onChange(대상 월 1일~말일) → `조회` 클릭 → 응답 대기(약 2~3초, 또는 카드 목록 갱신 감시).
5. 카드 목록에서 이름에 `동아리` 가 들어간 `li` 클릭 → `state.cardDataGridTab1.checkAll(true)`.
6. `확인` 은 사용자 확인 후(차단 요소 1).
7. 지출내역 각 행(`grid.getRowCount()`)에 `cashType(row, 용도 2204)` + `setValue(row,'rmkDc','동아리명_동아리활동')`.
8. `결재상신` 이후는 사용자가 진행(두 번째 화면 미확인).

```js
// 지출결의 페이지 컴포넌트와 그리드 찾기
function getExpendPage() {
  let el = document.querySelector('canvas');
  while (el && !Object.keys(el).some(k => k.startsWith('__reactInternalInstance'))) el = el.parentElement;
  let f = el[Object.keys(el).find(k => k.startsWith('__reactInternalInstance'))];
  while (f && !(f.stateNode && f.stateNode.handle && f.stateNode.handle.change)) f = f.return;
  return f && f.stateNode; // .state.component.grid = 지출내역 그리드
}
// 카드 팝업 컴포넌트 찾기 (팝업이 열린 뒤)
function getCardDialog() {
  const el = document.querySelector('.dialog_data input');
  let f = el[Object.keys(el).find(k => k.startsWith('__reactInternalInstance'))];
  while (f && !(f.stateNode && f.stateNode.onClickCard)) f = f.return;
  return f && f.stateNode; // .state.cardDataGridTab1 = 사용 내역 그리드, .state.Tab1.cardList = 카드 목록
}
```

### 관찰한 API(읽기용)

- `POST /personal/APB1020New/0ap00028` — 양식 설정(formId 201)
- `POST /personal/APCodePicker/ApAcashcdTypeCode` — 용도 코드 목록
- `POST /personal/APB1020New/0ap00008`, `0ap00029`, `0ap00030` — 카드 목록·카드 사용 내역 조회(일부 바디 암호화)
- `POST /system/financialApi/selectGisuData` — 회계처리일자 기수
- `POST /personal/APB1020New/0ap00001` — 기존 지출결의 문서 목록
- (쓰기, 호출 안 함) `POST /personal/APB1020/0ap01004` — 카드 반영 시 이미지 처리 로그
