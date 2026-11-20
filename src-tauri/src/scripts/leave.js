  // 연차휴가신청서 채우기. `params` = { kind: '연차'|'오전반차'|'오후반차', from: 'YYYYMMDD', to: 'YYYYMMDD', reason: '' }
  // 하단 버튼은 누르지 않습니다. 사용자가 확인한 뒤 직접 누릅니다.

  function leaveRow(label) {
    return rowByHeader(document, function(text) { return text === label; });
  }

  /** 근태신청서에 처음 들어가면 뜨는 「공지사항」 모달을 X 로 닫습니다. 없으면 그냥 넘어갑니다. */
  async function closeNotice() {
    var title = await waitUntil(function() {
      return Array.prototype.find.call(document.querySelectorAll('h1'), function(h1) {
        return isVisible(h1) && textOf(h1) === '공지사항';
      });
    }, 3000).catch(function() { return null; });
    if (!title) return;
    var box = title.parentElement && title.parentElement.parentElement;
    var close = box && box.querySelector('button');
    if (!close) throw new Error('공지사항 닫기 버튼을 찾을 수 없습니다');
    close.click();
    await waitUntil(function() { return !isVisible(title) || !document.contains(title); }, 3000, '공지사항이 닫히지 않았습니다');
  }

  runDraft(function() { return !!leaveRow('휴가구분'); }, async function() {
    await step('공지사항 닫기', closeNotice);

    await step('휴가구분', async function() {
      var row = leaveRow('휴가구분');
      var button = row && Array.prototype.find.call(row.querySelectorAll('button'), function(candidate) {
        return textOf(candidate) === params.kind;
      });
      if (!button) throw new Error('「' + params.kind + '」 버튼을 찾을 수 없습니다');
      button.click();
      await waitUntil(function() { return /focus/i.test(button.className); }, 5000, '휴가구분이 선택되지 않았습니다');
      await waitForIdle(10000);
    });

    await step('휴가기간', async function() {
      var row = leaveRow('휴가기간');
      var input = row && row.querySelector('input[class*="OBTDatePickerRebuild_inputYMD"]');
      if (!input) throw new Error('휴가기간 입력칸을 찾을 수 없습니다');
      setDateRange(input, params.from, params.to);
      await waitUntil(function() {
        var inputs = leaveRow('휴가기간').querySelectorAll('input[class*="OBTDatePickerRebuild_inputYMD"]');
        return inputs.length >= 2 && inputs[0].value === displayDate(params.from) && inputs[1].value === displayDate(params.to);
      }, 5000, '휴가기간이 반영되지 않았습니다');
      await waitForIdle(10000);
    });

    if (params.reason) {
      await step('비고(사유)', async function() {
        var row = leaveRow('비고(사유)');
        var input = row && row.querySelector('input:not([disabled])');
        if (!input) throw new Error('비고(사유) 입력칸을 찾을 수 없습니다');
        setValue(input, params.reason);
        await waitUntil(function() { return input.value === params.reason; }, 3000, '비고(사유)가 반영되지 않았습니다');
      });
    }
  });
