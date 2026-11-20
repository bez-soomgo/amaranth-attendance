  // 기안 양식 채우기 스크립트(leave.js, club.js)가 함께 쓰는 부분입니다. helpers.js 뒤, 양식별 스크립트 앞에
  // 이어 붙습니다. 진행 상황은 페이지 전역 `window.__amaranthDraftStatus` 에만 적고, 앱은 이 값을
  // 읽어 가기만 합니다(페이지에 앱 기능을 노출하지 않음).
  //
  // 이 스크립트는 제출 계열 버튼을 절대 누르지 않습니다(금지 글자 목록과 검사는 draft.rs 테스트).

  function report(state, step, message) {
    window.__amaranthDraftStatus = { state: state, step: step || '', message: message || '' };
  }

  /** 단계 이름을 상태에 적고 실행합니다. 실패하면 단계 이름과 함께 예외를 다시 던집니다. */
  async function step(name, run) {
    report('filling', name, '');
    try {
      return await run();
    } catch (error) {
      var failure = new Error(error && error.message ? error.message : String(error));
      failure.stepName = name;
      throw failure;
    }
  }

  /** 진행 중인 XHR·fetch 요청 수를 셉니다. 조회 버튼을 누른 뒤 응답이 끝났는지 판단할 때 씁니다. */
  function trackRequests() {
    if (window.__amaranthPendingRequests !== undefined) return;
    window.__amaranthPendingRequests = 0;
    window.__amaranthLastRequestAt = Date.now();
    var begin = function() { window.__amaranthPendingRequests += 1; window.__amaranthLastRequestAt = Date.now(); };
    var end = function() {
      window.__amaranthPendingRequests = Math.max(0, window.__amaranthPendingRequests - 1);
      window.__amaranthLastRequestAt = Date.now();
    };
    var send = XMLHttpRequest.prototype.send;
    XMLHttpRequest.prototype.send = function() {
      begin();
      this.addEventListener('loadend', end);
      return send.apply(this, arguments);
    };
    if (window.fetch) {
      var originalFetch = window.fetch;
      window.fetch = function() {
        begin();
        return originalFetch.apply(this, arguments).finally(end);
      };
    }
  }

  /** 요청이 하나도 없이 `quietMs` 동안 조용해질 때까지 기다립니다. */
  function waitForIdle(timeoutMs, quietMs) {
    var quiet = quietMs || 600;
    return waitUntil(function() {
      return window.__amaranthPendingRequests === 0 && Date.now() - window.__amaranthLastRequestAt >= quiet;
    }, timeoutMs, '그룹웨어 응답을 기다리다 시간이 지났습니다');
  }

  function textOf(element) {
    return (element && element.textContent || '').replace(/\s+/g, ' ').trim();
  }

  /** `th` 글자로 표의 행(`tr`)을 찾습니다. `match` 는 글자를 받아 참/거짓을 돌려주는 함수입니다. */
  function rowByHeader(root, match) {
    var header = Array.prototype.find.call((root || document).querySelectorAll('th'), function(th) {
      return isVisible(th) && match(textOf(th));
    });
    return header ? header.closest('tr') : null;
  }

  /** 글자가 정확히 `label` 인 보이는 버튼을 찾습니다. */
  function buttonByText(root, label) {
    return Array.prototype.find.call((root || document).querySelectorAll('button'), function(button) {
      return isVisible(button) && textOf(button) === label;
    }) || null;
  }

  /** OBT 기간 날짜 선택기의 값을 fiber 의 `onChange({value:{from,to}})` 로 바꿉니다(YYYYMMDD). */
  function setDateRange(input, from, to) {
    var fiber = findFiber(input, function(candidate) {
      var props = candidate.memoizedProps;
      return props && props.value && props.value.from !== undefined && typeof props.onChange === 'function';
    });
    if (!fiber) throw new Error('날짜 입력 컴포넌트를 찾을 수 없습니다');
    fiber.memoizedProps.onChange({ value: { from: from, to: to } });
  }

  function displayDate(yyyymmdd) {
    return yyyymmdd.slice(0, 4) + '-' + yyyymmdd.slice(4, 6) + '-' + yyyymmdd.slice(6, 8);
  }

  /** 양식(`isReady()`)이나 로그인 화면이 나타날 때까지 기다립니다. 로그인 화면이면 'login' 을 돌려줍니다. */
  function waitForFormOrLogin(isReady, timeoutMs) {
    return waitUntil(function() {
      if (isLoginPage()) return 'login';
      return isReady() ? 'form' : null;
    }, timeoutMs, '양식 화면이 열리지 않았습니다');
  }

  /** 한 페이지에서 한 번만 실행합니다. 로그인 화면이면 상태를 비워 두고 멈춥니다(앱이 로그인 후 다시 엽니다). */
  async function runDraft(isReady, fill) {
    if (window.__amaranthDraftStarted) return;
    window.__amaranthDraftStarted = true;
    trackRequests();
    try {
      var page = await step('양식 열기', function() { return waitForFormOrLogin(isReady, 40000); });
      if (page === 'login') {
        window.__amaranthDraftStatus = null;
        window.__amaranthDraftStarted = false;
        return;
      }
      var result = await fill();
      if (result && result.state) report(result.state, result.step, result.message);
      else report('done', '', '');
    } catch (error) {
      report('failed', error.stepName || '', error.message || String(error));
    }
  }
