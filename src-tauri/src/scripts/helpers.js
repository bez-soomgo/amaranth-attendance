  // 그룹웨어 웹뷰에 넣는 스크립트들이 함께 쓰는 도우미입니다. 이 파일은 단독으로 실행하지 않고
  // Rust 쪽에서 `(function() { ... })()` 안에 이어 붙입니다.

  /** React(OBT) 텍스트 입력은 `value` 대입만으로 반영되지 않아 native setter + 이벤트로 채웁니다. */
  function setValue(input, value) {
    var setter = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, 'value').set;
    setter.call(input, value);
    input.dispatchEvent(new Event('input', { bubbles: true }));
    input.dispatchEvent(new Event('change', { bubbles: true }));
  }

  /** `check()` 가 값을 돌려줄 때까지 기다립니다. 시간이 지나면 `onTimeout` 을 부릅니다(없으면 조용히 멈춤). */
  function waitFor(check, timeoutMs, intervalMs, onReady, onTimeout) {
    var waited = 0;
    var timer = setInterval(function() {
      var target = null;
      try { target = check(); } catch (error) { target = null; }
      if (target) {
        clearInterval(timer);
        onReady(target);
      } else if ((waited += intervalMs) >= timeoutMs) {
        clearInterval(timer);
        if (onTimeout) onTimeout();
      }
    }, intervalMs);
  }

  /** `waitFor` 의 Promise 판. 시간이 지나면 `message` 로 실패합니다. */
  function waitUntil(check, timeoutMs, message) {
    return new Promise(function(resolve, reject) {
      waitFor(check, timeoutMs, 100, resolve, function() {
        reject(new Error(message || '응답을 기다리다 시간이 지났습니다'));
      });
    });
  }

  function sleep(ms) {
    return new Promise(function(resolve) { setTimeout(resolve, ms); });
  }

  /** DOM 요소에 붙은 React fiber 를 돌려줍니다(React 16 `__reactInternalInstance$…`, 17+ `__reactFiber$…`). */
  function reactFiber(element) {
    if (!element) return null;
    var key = Object.keys(element).find(function(name) {
      return name.indexOf('__reactInternalInstance') === 0 || name.indexOf('__reactFiber') === 0;
    });
    return key ? element[key] : null;
  }

  /** 요소의 fiber 에서 부모 방향으로 올라가며 `predicate(fiber)` 를 만족하는 첫 fiber 를 찾습니다. */
  function findFiber(element, predicate) {
    for (var fiber = reactFiber(element); fiber; fiber = fiber.return) {
      if (predicate(fiber)) return fiber;
    }
    return null;
  }

  /** 요소가 화면에 보이는지(숨김 처리되지 않았는지) 확인합니다. */
  function isVisible(element) {
    return !!element && element.getClientRects().length > 0;
  }

  /** 로그인 화면인지 확인합니다. 세션이 없으면 양식 주소로 들어와도 로그인 화면으로 이동합니다. */
  function isLoginPage() {
    return location.hash.indexOf('login') >= 0 || location.pathname.indexOf('login') >= 0 ||
      !!document.querySelector('#reqLoginId');
  }
