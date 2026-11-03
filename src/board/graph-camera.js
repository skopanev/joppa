// The camera of the board graph: the viewBox of the SVG. As in a design tool:
// - A wheel or a two-finger swipe pans the view.
// - A pinch (a wheel with ctrlKey or metaKey, or a Safari gesture) zooms round the pointer.
// - fit() moves the view to a set of points. It eases over a few frames, or jumps at once
//   under reduced motion.
const MIN_WIDTH=60,MAX_WIDTH=60000;
export function graphCamera(canvas){
  let view=[-500,-500,1000,1000],goal=null,frame=0,box=null,boxAt=0;
  const calm=matchMedia('(prefers-reduced-motion: reduce)').matches;
  // The canvas box is read at most once in 500 ms. A scroll or a resize reads it again.
  const rect=()=>{const now=performance.now();if(!box||now-boxAt>500){box=canvas.getBoundingClientRect();boxAt=now;}return box;};
  for(const type of ['scroll','resize'])addEventListener(type,()=>{box=null;},{passive:true,capture:true});
  new ResizeObserver(()=>{box=null;}).observe(canvas);
  // The world units for one screen pixel. The SVG fits the whole view box into the canvas
  // (xMidYMid meet), so the larger of the two ratios applies.
  const unit=()=>{const b=rect();return b.width&&b.height?Math.max(view[2]/b.width,view[3]/b.height):1;};
  const apply=()=>{
    const u=unit();
    canvas.setAttribute('viewBox',view.map(v=>Math.round(v*100)/100).join(' '));
    canvas.style.setProperty('--u',u.toFixed(3));
    camera.onView?.(u);
  };
  const shape=([x,y,w,h])=>{
    const b=rect(),ratio=b.width&&b.height?b.width/b.height:1;
    if(w/h>ratio){const height=w/ratio;return [x,y-(height-h)/2,w,height];}
    const width=h*ratio;return [x-(width-w)/2,y,width,h];
  };
  const ease=()=>{
    frame=0;if(!goal)return;
    view=view.map((v,i)=>v+(goal[i]-v)*0.25);
    if(view.every((v,i)=>Math.abs(goal[i]-v)<goal[2]*0.002)){view=goal;goal=null;}
    apply();if(goal)frame=requestAnimationFrame(ease);
  };
  const stop=()=>{goal=null;cancelAnimationFrame(frame);frame=0;};
  // The world point under the pointer.
  const point=event=>{
    const b=rect();if(!b.width||!b.height)return [view[0]+view[2]/2,view[1]+view[3]/2];
    const u=unit();
    return [view[0]+view[2]/2+(event.clientX-b.left-b.width/2)*u,view[1]+view[3]/2+(event.clientY-b.top-b.height/2)*u];
  };
  const zoom=(event,k)=>{
    stop();const [x,y]=point(event),w=Math.min(MAX_WIDTH,Math.max(MIN_WIDTH,view[2]*k)),s=w/view[2];
    view=[x-(x-view[0])*s,y-(y-view[1])*s,w,view[3]*s];apply();
  };
  const camera={
    point,stop,
    view:()=>[...view],
    // Move the view by a screen distance in pixels.
    pan(dx,dy){stop();const u=unit();view=[view[0]+dx*u,view[1]+dy*u,view[2],view[3]];apply();},
    // Show every point with a margin. Fewer than one point keeps the view.
    fit(points,margin=48){
      if(!points.length)return;
      let x0=Infinity,y0=Infinity,x1=-Infinity,y1=-Infinity;
      for(const p of points){x0=Math.min(x0,p.x);y0=Math.min(y0,p.y);x1=Math.max(x1,p.x);y1=Math.max(y1,p.y);}
      const pad=margin*Math.max(1,(x1-x0)/800),w=Math.max(x1-x0+2*pad,240),h=Math.max(y1-y0+2*pad,160);
      goal=shape([(x0+x1)/2-w/2,(y0+y1)/2-h/2,w,h]);
      if(calm){view=goal;goal=null;apply();return;}
      if(!frame)frame=requestAnimationFrame(ease);
    },
  };
  canvas.addEventListener('wheel',event=>{
    event.preventDefault();
    const lines=event.deltaMode===1?16:event.deltaMode===2?canvas.clientHeight:1;
    if(event.ctrlKey||event.metaKey)zoom(event,Math.exp(event.deltaY*lines*0.01));
    else camera.pan(event.deltaX*lines,event.deltaY*lines);
    camera.onMove?.();
  },{passive:false});
  // Safari sends the pinch of a trackpad as gesture events with a scale.
  let scale=1;
  canvas.addEventListener('gesturestart',event=>{event.preventDefault();scale=1;});
  canvas.addEventListener('gesturechange',event=>{event.preventDefault();zoom(event,scale/event.scale);scale=event.scale;camera.onMove?.();});
  apply();
  return camera;
}
