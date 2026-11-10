// The camera of Flow view: the viewBox of the SVG. As in a design tool:
// - A wheel or a two-finger swipe pans the view.
// - A pinch (a wheel with ctrlKey or metaKey, or a Safari gesture) zooms round the pointer.
// - fit() moves the view to a set of points at once.
const MIN_WIDTH=60,MAX_WIDTH=60000;
export function flowCamera(canvas){
  let view=[-500,-500,1000,1000],box=null,boxAt=0;
  // The canvas box is read at most once in 500 ms. A scroll or a resize reads it again.
  const rect=()=>{const now=performance.now();if(!box||now-boxAt>500){box=canvas.getBoundingClientRect();boxAt=now;}return box;};
  // destroy() removes the window listeners and the observer, so the window keeps no old canvas.
  const stop=new AbortController(),observer=new ResizeObserver(()=>{box=null;});
  for(const type of ['scroll','resize'])addEventListener(type,()=>{box=null;},{passive:true,capture:true,signal:stop.signal});
  observer.observe(canvas);
  // The world units for one screen pixel. The SVG fits the whole view box into the canvas
  // (xMidYMid meet), so the larger of the two ratios applies.
  const unit=()=>{const b=rect();return b.width&&b.height?Math.max(view[2]/b.width,view[3]/b.height):1;};
  const apply=()=>{
    canvas.setAttribute('viewBox',view.map(v=>Math.round(v*100)/100).join(' '));
    canvas.style.setProperty('--u',unit().toFixed(3));
  };
  const shape=([x,y,w,h])=>{
    const b=rect(),ratio=b.width&&b.height?b.width/b.height:1;
    if(w/h>ratio){const height=w/ratio;return [x,y-(height-h)/2,w,height];}
    const width=h*ratio;return [x-(width-w)/2,y,width,h];
  };
  // The world point under the pointer.
  const point=event=>{
    const b=rect();if(!b.width||!b.height)return [view[0]+view[2]/2,view[1]+view[3]/2];
    const u=unit();
    return [view[0]+view[2]/2+(event.clientX-b.left-b.width/2)*u,view[1]+view[3]/2+(event.clientY-b.top-b.height/2)*u];
  };
  const zoom=(event,k)=>{
    const [x,y]=point(event),w=Math.min(MAX_WIDTH,Math.max(MIN_WIDTH,view[2]*k)),s=w/view[2];
    view=[x-(x-view[0])*s,y-(y-view[1])*s,w,view[3]*s];apply();
  };
  const camera={
    point,unit,
    view:()=>[...view],
    destroy(){stop.abort();observer.disconnect();},
    // Move the view by a screen distance in pixels.
    pan(dx,dy){const u=unit();view=[view[0]+dx*u,view[1]+dy*u,view[2],view[3]];apply();},
    // Show every point with a margin, centred. Fewer than one point keeps the view.
    fit(points,margin=60){
      if(!points.length)return;
      let x0=Infinity,y0=Infinity,x1=-Infinity,y1=-Infinity;
      for(const p of points){x0=Math.min(x0,p.x);y0=Math.min(y0,p.y);x1=Math.max(x1,p.x);y1=Math.max(y1,p.y);}
      const w=Math.max(x1-x0+2*margin,240),h=Math.max(y1-y0+2*margin,160);
      view=shape([(x0+x1)/2-w/2,(y0+y1)/2-h/2,w,h]);
      apply();
    },
  };
  canvas.addEventListener('wheel',event=>{
    event.preventDefault();
    const lines=event.deltaMode===1?16:event.deltaMode===2?canvas.clientHeight:1;
    if(event.ctrlKey||event.metaKey)zoom(event,Math.exp(event.deltaY*lines*0.01));
    else camera.pan(event.deltaX*lines,event.deltaY*lines);
  },{passive:false});
  // Safari sends the pinch of a trackpad as gesture events with a scale.
  let scale=1;
  canvas.addEventListener('gesturestart',event=>{event.preventDefault();scale=1;});
  canvas.addEventListener('gesturechange',event=>{event.preventDefault();zoom(event,scale/event.scale);scale=event.scale;});
  apply();
  return camera;
}
