(function() {
    const existingBar = document.getElementById('__zenith_find__');
    if (existingBar) {
        const inp = existingBar.querySelector('input');
        if (inp) { inp.focus(); inp.select(); }
        return;
    }
    
    // Create the host element with a shadow DOM to isolate from page CSS
    const host = document.createElement('div');
    host.id = '__zenith_find__';
    host.style.cssText = 'all:initial;position:fixed;top:16px;right:16px;z-index:2147483647;';
    document.documentElement.appendChild(host);
    
    const shadow = host.attachShadow({ mode: 'open' });
    shadow.innerHTML = `
        <style>
            :host { all: initial; }
            .bar {
                display: flex; align-items: center; gap: 6px;
                background: rgba(25,25,35,0.97);
                border: 1px solid rgba(100,120,255,0.4);
                border-radius: 12px; padding: 7px 10px;
                box-shadow: 0 12px 40px rgba(0,0,0,0.6);
                backdrop-filter: blur(24px);
                font-family: -apple-system, BlinkMacSystemFont, sans-serif;
            }
            input {
                background: rgba(255,255,255,0.12);
                border: 1px solid rgba(255,255,255,0.2);
                border-radius: 7px; padding: 6px 11px;
                color: #fff; font-size: 13px; width: 190px;
                outline: none; font-family: inherit;
            }
            input:focus { border-color: rgba(100,140,255,0.7); }
            input::placeholder { color: rgba(255,255,255,0.4); }
            button {
                background: rgba(255,255,255,0.08);
                border: 1px solid rgba(255,255,255,0.14);
                border-radius: 7px; color: #ccc;
                padding: 5px 9px; cursor: pointer;
                font-size: 13px; font-family: inherit;
                transition: background 0.1s;
            }
            button:hover { background: rgba(255,255,255,0.18); color: #fff; }
            .count { color: rgba(255,255,255,0.5); font-size: 11px; min-width: 44px; text-align: center; }
            .close { background: transparent; border: none; font-size: 17px; color: rgba(255,255,255,0.5); padding: 2px 6px; }
            .close:hover { color: #f87171; background: rgba(248,113,113,0.15); }
        </style>
        <div class="bar">
            <input id="findinput" placeholder="Find in page..." autocomplete="off" spellcheck="false" />
            <button id="prev">↑</button>
            <button id="next">↓</button>
            <span class="count" id="count"></span>
            <button class="close" id="close">✕</button>
        </div>
    `;
    
    const inp = shadow.getElementById('findinput');
    const countEl = shadow.getElementById('count');
    let lastQuery = '';
    
    function doFind(q, forward) {
        if (!q) { window.getSelection() && window.getSelection().removeAllRanges(); countEl.textContent = ''; return; }
        if (q !== lastQuery) {
            window.getSelection() && window.getSelection().removeAllRanges();
            lastQuery = q;
        }
        const found = window.find(q, false, !forward, true, false, false, false);
        countEl.textContent = found ? '✓ Found' : '✗ Not found';
        // Reclaim focus after window.find() moves it to the highlighted text
        setTimeout(function() { inp.focus(); }, 0);
    }
    
    // Do NOT search on input - only on Enter / button click
    // (live search causes focus-reset loop after every character)
    inp.addEventListener('keydown', function(e) {
        e.stopPropagation();
        e.stopImmediatePropagation();
        if (e.key === 'Enter') { doFind(inp.value, !e.shiftKey); e.preventDefault(); }
        if (e.key === 'Escape') { host.remove(); e.preventDefault(); }
    }, true);
    
    shadow.getElementById('next').addEventListener('click', function() { doFind(inp.value, true); });
    shadow.getElementById('prev').addEventListener('click', function() { doFind(inp.value, false); });
    shadow.getElementById('close').addEventListener('click', function() { host.remove(); });
    
    setTimeout(function() { inp.focus(); }, 30);
})();
