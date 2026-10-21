// Line icons drawn as inline SVG.
export const FILE_ICON='M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8Z M14 2v6h6 M8 13h8 M8 17h5';
function lineIcon(paths){
  const ns='http://www.w3.org/2000/svg',svg=document.createElementNS(ns,'svg');
  for(const [key,value] of Object.entries({viewBox:'0 0 24 24',width:'14',height:'14',fill:'none',stroke:'currentColor','stroke-width':'1.6','stroke-linecap':'round','stroke-linejoin':'round','aria-hidden':'true',focusable:'false'}))svg.setAttribute(key,value);
  for(const d of paths){const path=document.createElementNS(ns,'path');path.setAttribute('d',d);svg.append(path);}
  return svg;
}
export const historyIcon=()=>lineIcon(['M3 12a9 9 0 1 0 3-6.7','M3 4v4h4','M12 7v5l3 2']);
export const propertiesIcon=()=>lineIcon(['M21 12a9 9 0 1 1-18 0a9 9 0 0 1 18 0','M12 16v-5','M12 8h.01']);
export function svgIcon(d,size=16){
  const icon=document.createElementNS('http://www.w3.org/2000/svg','svg');
  for(const [k,v] of Object.entries({viewBox:'0 0 24 24',width:String(size),height:String(size),fill:'none',stroke:'currentColor','stroke-width':'1.6','stroke-linecap':'round','stroke-linejoin':'round','aria-hidden':'true'}))icon.setAttribute(k,v);
  const path=document.createElementNS(icon.namespaceURI,'path');path.setAttribute('d',d);icon.append(path);
  return icon;
}
