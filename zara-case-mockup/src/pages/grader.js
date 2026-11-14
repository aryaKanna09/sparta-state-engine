'use strict';
const L = require('../lib/layout');
const D = require('../data');
const { COPY, CHECKS } = D;
const { header, footer, note, phTemplates } = L;

/* ====================================================================== 13
   THE LIVE GRADER — a working camera demo, not a mockup of one.
   ====================================================================== */
function grader() {
  const dots = '<i class="on"></i><i></i><i></i><i></i>';

  return `
${header({ nav: 'WOMAN' })}
<main class="stage">
  <div class="stage__intro">
    <p class="u-eyebrow">${COPY.featureName} &middot; LIVE DEMO</p>
    <h1 class="big">Point a camera at a jacket. Watch it get graded.</h1>
    <p class="lede">This one is real. It opens the camera, reads every frame for exposure, sharpness and framing,
      and only arms the shutter when the shot is good enough to grade from. Works on a laptop webcam or a phone.</p>
  </div>

  <div class="phones">
    <div>
      <div class="phone phone--live">
        <div class="phone__cap"><span>9:41</span><span class="phone__sig"><i></i><i></i><i></i><i></i></span></div>
        <div class="phone__screen">

          <!-- ---------------------------------------------------- start -->
          <section id="v-start" class="gview">
            <div class="app-bar"><span class="app-bar__back">&#8249;</span>
              <span class="app-bar__title">SELL THIS ITEM</span><span class="app-bar__back">&nbsp;</span></div>
            <div class="app-body app-body--center">
              <div class="glens"></div>
              <p class="u-label" style="margin-top:20px">FOUR PHOTOGRAPHS</p>
              <p class="u-meta" style="margin-top:8px;max-width:26ch">
                Front, back, the Zara tag, and any wear. We coach each one, then grade it.</p>
              <p id="startmsg" class="u-meta" style="margin-top:14px;color:var(--grey-400)"></p>
            </div>
            <div class="app-foot">
              <button id="enable" class="btn btn--primary btn--block">ENABLE CAMERA</button>
              <p class="u-meta" style="text-align:center">Nothing leaves your device.</p>
            </div>
          </section>

          <!-- -------------------------------------------------- capture -->
          <section id="v-capture" class="gview" hidden>
            <div class="app-bar"><span class="app-bar__back">&#8249;</span>
              <span class="app-bar__title">PHOTOGRAPH THE ITEM</span>
              <span class="app-bar__title" id="cstep">01 / 04</span></div>
            <div class="cam cam--live">
              <video id="cam" playsinline muted autoplay></video>
              <img id="still" alt="" style="display:none">
              <div class="cam__frame"></div>
              <div class="cam__steps" id="cdots">${dots}</div>
              <div class="gchips">
                <span class="gchip" id="chip-light">LIGHT</span>
                <span class="gchip" id="chip-sharp">SHARP</span>
                <span class="gchip" id="chip-frame">FRAME</span>
              </div>
              <div class="cam__coach">
                <b id="clabel">FRONT, FLAT, DAYLIGHT</b>
                <p id="ccoach">Lay the blazer flat near a window.</p>
                <p id="readout" class="greadout"></p>
              </div>
              <div class="gthumbs" id="thumbs"></div>
              <button class="cam__shutter" id="shutter" aria-label="Take photograph"></button>
            </div>
          </section>

          <!-- -------------------------------------------------- grading -->
          <section id="v-grading" class="gview" hidden>
            <div class="app-bar"><span class="app-bar__back">&nbsp;</span>
              <span class="app-bar__title">GRADING</span><span class="app-bar__back">&nbsp;</span></div>
            <div class="app-body app-body--center">
              <div class="spinner"></div>
              <p class="u-label" style="margin-top:22px">READING YOUR PHOTOGRAPHS</p>
              <ul class="ticks" id="ticks">
                ${CHECKS.map(([k]) => `<li>${k}</li>`).join('')}
              </ul>
            </div>
          </section>

          <!-- ---------------------------------------------------- offer -->
          <section id="v-offer" class="gview" hidden>
            <div class="app-bar"><span class="app-bar__back">&#8249;</span>
              <span class="app-bar__title">YOUR OFFER</span>
              <span class="app-bar__back">&nbsp;</span></div>
            <div class="app-body">
              <div class="offer">
                <div class="offer__tier" id="otier"></div>
                <div class="offer__amt" id="oamt"></div>
                <div class="offer__sub" id="oblurb"></div>
              </div>
              <div class="band">
                <div class="band__track">
                  <div class="band__fill" id="ofill" style="left:0"></div>
                  <div class="band__pin" id="opin"></div>
                </div>
                <div class="band__ends"><span id="oband"></span></div>
              </div>
              <p class="u-meta" id="oqual"></p>
              <div class="gshots" id="oshots"></div>
              <div class="checks" style="margin-top:16px">
                ${CHECKS.map(([k, v]) => `<div class="check"><span class="check__k">${k}</span><span class="check__v">${v}</span></div>`).join('')}
              </div>
              <p class="u-meta" style="margin-top:12px">The grade is simulated for this demo. The capture coaching above is real.</p>
            </div>
            <div class="app-foot">
              <button class="btn btn--primary btn--block" id="oaccept">ACCEPT</button>
              <div class="foot-row">
                <a class="link-u" href="/seller-appeal.html">Appeal the grade</a>
                <button class="link-u" id="again">Start over</button>
              </div>
            </div>
          </section>

        </div>
      </div>
      <div class="phone__label">LIVE CAMERA<span id="forcedtag" hidden class="glock"></span></div>
    </div>
  </div>

  ${note('HOW TO RUN IT ON STAGE', `Open this screen on a phone and hold a jacket up, or use the laptop webcam.
    The three chips go black when exposure, sharpness and framing pass — that part is genuinely measuring the frame.
    <b>Press 1, 2 or 3 before the fourth shot to lock the tier</b> to EXCELLENT, GOOD or FAIR so the result is
    predictable in front of judges. Space bar fires the shutter. If the camera is blocked, it falls back to a bundled
    photograph and still completes the flow.`)}

  ${note('WHAT IS REAL AND WHAT IS NOT', `Real: camera capture, and per-frame analysis of exposure, Laplacian
    sharpness and edge-framing, computed in the browser. Simulated: the condition grade itself. It is weighted by
    measured capture quality so it reacts to what you shoot, but it is not a trained condition model — say so if a
    judge asks, because the honest version of this answer is stronger than the vague one.`)}
</main>
${footer()}
${phTemplates()}`;
}

module.exports = { grader };
