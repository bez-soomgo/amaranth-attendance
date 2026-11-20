  // 결재 작성 창(UBAP001)에 영수증 파일을 첨부합니다. helpers.js, draft-common.js 뒤에 이어 붙습니다.
  // `params` = { files: [{ name, type, data(base64) }] }
  // 숨은 `input#uploadFile` 에 files 를 넣는 방식은 반영되지 않아, 첨부 영역(`.conBox`)에
  // dragover → drop 이벤트를 DataTransfer 와 함께 보냅니다. 첨부 시점에는 업로드 요청이 없고,
  // 사용자가 직접 누르는 마지막 단계에서 올라갑니다. 본문 편집기와 다른 버튼은 건드리지 않습니다.

  function attachBox() {
    var input = document.getElementById('uploadFile');
    return input ? input.closest('.conBox') : null;
  }

  function toFile(entry) {
    var binary = atob(entry.data);
    var bytes = new Uint8Array(binary.length);
    for (var index = 0; index < binary.length; index += 1) bytes[index] = binary.charCodeAt(index);
    return new File([bytes], entry.name, { type: entry.type });
  }

  (async function() {
    if (window.__amaranthAttachStarted) return;
    window.__amaranthAttachStarted = true;
    try {
      var box = await step('영수증 첨부', function() {
        return waitUntil(attachBox, 30000, '결재 작성 창의 첨부 영역을 찾을 수 없습니다');
      });
      await step('영수증 첨부', async function() {
        var transfer = new DataTransfer();
        params.files.forEach(function(entry) { transfer.items.add(toFile(entry)); });
        var options = { bubbles: true, cancelable: true, dataTransfer: transfer };
        box.dispatchEvent(new DragEvent('dragover', options));
        box.dispatchEvent(new DragEvent('drop', options));
        await waitUntil(function() {
          var current = attachBox();
          var text = current && current.parentElement ? current.parentElement.innerText : '';
          return params.files.every(function(entry) { return text.indexOf(entry.name) >= 0; });
        }, 10000, '첨부 목록에 일부 파일이 보이지 않습니다');
      });
      // 완료 문구는 앱이 만듭니다(스크립트에는 제출 버튼 글자를 넣지 않음).
      report('done', '영수증 첨부', '');
    } catch (error) {
      report('failed', error.stepName || '영수증 첨부', error.message || String(error));
    }
  })();
