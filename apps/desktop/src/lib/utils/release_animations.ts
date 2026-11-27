// WebKit never garbage collects an animation that still has a timeline, and cancel() alone
// does not free it: https://bugs.webkit.org/show_bug.cgi?id=329512
let installed = false;

export function installAnimationRelease() {
	if (installed) return;
	installed = true;

	const animate = Element.prototype.animate;

	Element.prototype.animate = function (...args) {
		const animation = animate.apply(this, args);
		// Deferred so the owner's own finish and cancel handlers run first.
		animation.addEventListener('finish', () => setTimeout(() => releaseFinished(animation)));
		animation.addEventListener('cancel', () => setTimeout(() => releaseCancelled(animation)));
		return animation;
	};
}

function releaseFinished(animation: Animation) {
	if (animation.playState !== 'finished') return;
	// A forwards fill still styles the element; its owner releases it by cancelling.
	const fill = animation.effect?.getTiming().fill;
	if (fill === 'forwards' || fill === 'both') return;
	animation.cancel();
	animation.timeline = null;
}

function releaseCancelled(animation: Animation) {
	if (animation.playState === 'idle') animation.timeline = null;
}
