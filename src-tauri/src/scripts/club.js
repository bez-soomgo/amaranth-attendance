  // 동아리 활동 내역서(지출결의서) 채우기.
  // `params` = { title, from: 'YYYYMMDD', to: 'YYYYMMDD', cardName, memo, cashType: {...}, confirmCards: true }
  // 카드사용내역 팝업의 「확인」까지만 누르고, 페이지 상단의 제출 계열 버튼은 누르지 않습니다.
  // `confirmCards` 가 false 면 전체 선택까지만 하고 멈춥니다(시험용).

  /** 지출결의 페이지 컴포넌트(`handle.change` 를 가진 것)를 찾습니다. `.state.component.grid` 가 지출내역 그리드입니다. */
  function expendPage() {
    var canvases = document.querySelectorAll('canvas');
    for (var index = 0; index < canvases.length; index += 1) {
      var element = canvases[index];
      while (element && !reactFiber(element)) element = element.parentElement;
      var fiber = findFiber(element, function(candidate) {
        return candidate.stateNode && candidate.stateNode.handle && candidate.stateNode.handle.change;
      });
      if (fiber) return fiber.stateNode;
    }
    return null;
  }

  function expendGrid() {
    var page = expendPage();
    return page && page.state && page.state.component && page.state.component.grid || null;
  }

  /** 보이는 「카드사용 내역」 팝업 루트(`.dialog_data`)를 찾습니다. */
  function cardDialogRoot() {
    return Array.prototype.find.call(document.querySelectorAll('.dialog_data'), function(root) {
      return isVisible(root) && textOf(root).indexOf('카드사용') >= 0 &&
        !!root.querySelector('input[class*="OBTDatePickerRebuild_inputYMD"]');
    }) || null;
  }

  /** 카드 팝업 컴포넌트(`onClickCard` 를 가진 것). `.state.cardDataGridTab1` 이 사용 내역 그리드입니다. */
  function cardDialog() {
    var root = cardDialogRoot();
    var fiber = root && findFiber(root.querySelector('input'), function(candidate) {
      return candidate.stateNode && candidate.stateNode.onClickCard;
    });
    return fiber ? fiber.stateNode : null;
  }

  function cardGrid() {
    var dialog = cardDialog();
    return dialog && dialog.state && dialog.state.cardDataGridTab1 || null;
  }

  function cardItem() {
    var root = cardDialogRoot();
    if (!root) return null;
    return Array.prototype.find.call(root.querySelectorAll('li'), function(item) {
      return isVisible(item) && textOf(item).indexOf(params.cardName) >= 0;
    }) || null;
  }

  /** 금액이 있는 지출내역 행 번호들. 금액 열을 읽을 수 없으면 모든 행을 돌려줍니다. */
  function filledRows(grid) {
    var count = grid.getRowCount();
    var rows = [];
    var readable = false;
    for (var row = 0; row < count; row += 1) {
      var amount = null;
      try { amount = grid.getValue(row, 'sumAm'); readable = true; } catch (error) { amount = null; }
      if (amount !== null && amount !== undefined && amount !== '' && Number(amount) !== 0) rows.push(row);
    }
    if (!readable) {
      rows = [];
      for (var all = 0; all < count; all += 1) rows.push(all);
    }
    return rows;
  }

  runDraft(function() { return !!buttonByText(document, '카드사용내역') && !!expendPage(); }, async function() {
    await step('제목', async function() {
      var row = rowByHeader(document, function(text) { return text === '제목'; });
      var input = row && row.querySelector('input:not([disabled])');
      if (!input) throw new Error('제목 입력칸을 찾을 수 없습니다');
      setValue(input, params.title);
      await waitUntil(function() { return input.value === params.title; }, 3000, '제목이 반영되지 않았습니다');
    });

    await step('카드사용내역 열기', async function() {
      buttonByText(document, '카드사용내역').click();
      await waitUntil(function() { return cardDialog(); }, 10000, '카드사용 내역 창이 열리지 않았습니다');
      await waitForIdle(15000);
    });

    await step('사용일자 설정', async function() {
      var input = cardDialogRoot().querySelector('input[class*="OBTDatePickerRebuild_inputYMD"]');
      setDateRange(input, params.from, params.to);
      await waitUntil(function() {
        var inputs = cardDialogRoot().querySelectorAll('input[class*="OBTDatePickerRebuild_inputYMD"]');
        return inputs.length >= 2 && inputs[0].value === displayDate(params.from) && inputs[1].value === displayDate(params.to);
      }, 5000, '사용일자가 반영되지 않았습니다');
    });

    await step('조회', async function() {
      var search = buttonByText(cardDialogRoot(), '조회');
      if (!search) throw new Error('조회 버튼을 찾을 수 없습니다');
      search.click();
      await sleep(300);
      await waitForIdle(20000);
    });

    await step('카드 선택', async function() {
      var item = await waitUntil(cardItem, 10000, '「' + params.cardName + '」 카드를 목록에서 찾을 수 없습니다');
      item.click();
      await sleep(300);
      await waitForIdle(20000);
    });

    var total = await step('카드 내역 확인', async function() {
      // 그리드가 늦게 그려질 수 있어 잠깐 더 기다립니다. 끝까지 0건이면 반영할 내역이 없는 것입니다.
      return waitUntil(function() {
        var grid = cardGrid();
        var count = grid ? grid.getRowCount() : 0;
        return count > 0 ? count : null;
      }, 3000).catch(function() { return 0; });
    });
    if (!total) {
      return { state: 'empty', step: '카드 내역 확인', message: '반영할 카드 내역이 없습니다' };
    }

    await step('전체 선택', async function() {
      var grid = cardGrid();
      grid.checkAll(true);
      if (typeof grid.getCheckedIndexes === 'function') {
        await waitUntil(function() { return grid.getCheckedIndexes().length === total; }, 3000, '카드 내역을 모두 선택하지 못했습니다');
      }
    });

    if (params.confirmCards === false) {
      return { state: 'done', step: '전체 선택', message: '시험 실행: 확인 전에 멈췄습니다' };
    }

    await step('카드 내역 반영', async function() {
      var confirm = buttonByText(cardDialogRoot(), '확인');
      if (!confirm) throw new Error('카드사용 내역 창의 확인 버튼을 찾을 수 없습니다');
      confirm.click();
      // 확인 뒤 그룹웨어가 카드 영수증 이미지를 만들어 붙이므로 시간이 걸릴 수 있습니다.
      await waitUntil(function() { return !cardDialogRoot(); }, 30000, '카드사용 내역 창이 닫히지 않았습니다');
      await waitUntil(function() {
        var grid = expendGrid();
        return grid && filledRows(grid).length >= total;
      }, 60000, '지출내역에 카드 내역이 들어오지 않았습니다');
      await waitForIdle(60000, 1000);
    });

    await step('용도·내용 입력', async function() {
      var page = expendPage();
      var grid = expendGrid();
      var rows = filledRows(grid);
      for (var index = 0; index < rows.length; index += 1) {
        var result = page.handle.change.cashType(rows[index], Object.assign({}, params.cashType));
        if (result && typeof result.then === 'function') await result;
        grid.setValue(rows[index], 'rmkDc', params.memo);
        await sleep(150);
      }
      await waitUntil(function() {
        return rows.every(function(row) {
          return grid.getValue(row, 'cashCd') === params.cashType.typeCd && grid.getValue(row, 'rmkDc') === params.memo;
        });
      }, 5000, '일부 행의 용도·내용이 반영되지 않았습니다');
      return rows.length;
    });
  });
