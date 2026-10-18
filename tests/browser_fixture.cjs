// The fixture data of tests/browser.cjs: the tree, the rows, the verification record and the
// object details that the mocked server returns.
const ATTACK='</script><script>window.attacked=true</script>';
const SUBJECT_ATTACK='<img src=x onerror=alert(1)>';
const types={
  area:{title:'Area',index:10,role:'group',root:true,parents:[]},
  spec:{title:'Spec',index:20,role:'requirement',root:false,parents:['area']},
  measure:{title:'Measure',index:30,role:'ac',root:false,parents:['spec']},
};
const schema={workspace:'w',current_revision:2,schema:{revision:2,actor:'owner',
  at:'2026-10-06T09:00:00Z',reason:'Configured tree',types}};

const object=(address,extra={})=>({address,kind:address.split(':')[0],id:address.split(':')[1],
  title:extra.title||'Object',revision:1,present:true,owner:'owner',at:'2026-10-06T09:00:00Z',
  work_ready:true,review_required:false,item_id:'w-O-ABCDEFGHIJ',links:[],
  parent:extra.parent??null,...extra});
const rows=[
  object('area:core',{title:'Core area'}),
  object('spec:access',{title:'Access spec',parent:'area:core',process:{req:'access',revision:3}}),
];
const old=new Date(Date.now()-14*24*60*60*1000).toISOString();
const record={revision:3,confirmed:true,
  verification_scope:{scope:'One build of the web client'},
  criteria:[
    {id:'m1',address:'measure:m1',title:'Sign in works',signal:{tone:'green',label:'Checks passed',stale_after:old}},
    {id:'m2',address:'measure:m2',title:'Session survives a refresh',signal:{tone:'red',label:'Checks failed'}},
    {id:'m3',address:'measure:m3',title:'Nothing linked yet',signal:{tone:'grey',label:'Not checked'}},
  ],
  runs:{r1:{scope:'One build of the web client',subject:{code:'code-sha-v1',environment:'pg16-image-v1',fixture:SUBJECT_ATTACK},plan:[
    {id:'t-pass[m1]',ac:'m1',method:'automated',expected:['a session cookie']},
    {id:'t-stale[m1]',ac:'m1',method:'automated',expected:['an older pass']},
    {id:'t-fail[m2]',ac:'m2',method:'automated',expected:['a kept session']},
    {id:'t-skip[m2]',ac:'m2',method:'manual',expected:['a manual look']},
  ],results:{
    't-pass[m1]':{outcome:'pass',observed:['a session cookie'],note:'Recorded by the suite'},
    't-stale[m1]':{outcome:'pass',observed:['an older pass'],note:'Recorded two weeks ago'},
    't-fail[m2]':{outcome:'fail',observed:['the session was dropped'],note:'Reproduced twice'},
    't-skip[m2]':{outcome:'skip',observed:[],note:''},
  }}},
  result_dates:{r1:{'t-pass[m1]':new Date().toISOString(),'t-stale[m1]':old}}};

const detail=address=>({workspace:'w',address,kind:address.split(':')[0],id:address.split(':')[1],
  item_id:'w-O-ABCDEFGHIJ',current_revision:1,revision_count:1,work_ready:true,review_required:false,
  process:rows.find(r=>r.address===address)?.process||null,
  document:{revision:1,parent:rows.find(r=>r.address===address)?.parent||null,present:true,links:[],
    data:{title:rows.find(r=>r.address===address)?.title||'Object',owner:'owner',body:ATTACK},
    actor:'owner',at:'2026-10-06T09:00:00Z',reason:'Create'},
  links:[],incoming:[],
  annotations:{comments:{},attachments:{f1:{name:'evidence.png',content_type:'image/png',
    size:2048,hash:'abc',actor:'owner',at:'2026-10-06T09:00:00Z',removed:false}}}});

module.exports={ATTACK,SUBJECT_ATTACK,types,schema,object,rows,old,record,detail};
