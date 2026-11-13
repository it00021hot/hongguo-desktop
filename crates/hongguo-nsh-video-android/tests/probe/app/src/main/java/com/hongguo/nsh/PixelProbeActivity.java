package com.hongguo.nsh;

import android.app.Activity;
import android.content.pm.ActivityInfo;
import android.os.Bundle;
import android.os.Handler;
import android.os.Looper;
import android.util.Log;
import android.widget.FrameLayout;
import android.view.TextureView;
import android.graphics.Bitmap;
import android.media.MediaPlayer;
import java.io.File;
import java.nio.file.Files;
import java.util.Map;
import org.json.JSONObject;

/** Offline synthetic media only; instantiates the exact production ABI component. */
public final class PixelProbeActivity extends Activity {
    static { System.loadLibrary("nsh"); }
    private final Handler handler = new Handler(Looper.getMainLooper());
    private PixelVideoComponent video;
    private WindowModeComponent orientation;
    private JSONObject props;
    private int step;
    private int loads;
    private MediaPlayer firstPlayer;
    private int pausedPosition;
    private boolean pausedGpuReady;
    private boolean playingGpuReady;
    private boolean animeGpuReady;
    private boolean completed;
    private boolean replayOnly;
    private MediaPlayer replayPlayer;
    @Override public void onCreate(Bundle saved) {
        super.onCreate(saved);
        setRequestedOrientation(ActivityInfo.SCREEN_ORIENTATION_SENSOR_PORTRAIT);
        replayOnly=getIntent().getBooleanExtra("replayOnly",false);
        if(replayOnly)step=20;
        try {
            File sample = new File(getFilesDir(), "input.mp4");
            try (var input = getAssets().open("input.mp4")) { Files.copy(input, sample.toPath(), java.nio.file.StandardCopyOption.REPLACE_EXISTING); }
            props = new JSONObject("{\"source\":null,\"paused\":false,\"muted\":true,\"volume\":1,\"rate\":1,\"resizeMode\":\"contain\",\"progressIntervalMs\":100,\"enhancement\":{\"anime4kOn\":false,\"sharpenOn\":false,\"strength\":50},\"active\":true}");
            props.put("source", new JSONObject().put("id", "offline-1").put("url", sample.toURI().toString()));
            video = new PixelVideoComponent(this, this::event);
            FrameLayout root = new FrameLayout(this);
            root.addView(video.view(), new FrameLayout.LayoutParams(-1,-1));
            orientation = new WindowModeComponent(this, this::orientationEvent);
            root.addView(orientation.view(), new FrameLayout.LayoutParams(1,1));
            setContentView(root);
            update();
            handler.postDelayed(() -> { if (!completed) fail("probe deadline step="+step); }, 45_000);
        } catch (Exception error) { fail(error.toString()); }
    }
    private void mark(String name) { Log.i("HGPixelProbe", name); }
    private void inspect(String name, boolean enhanced) {
        run(() -> {
            FrameLayout root=(FrameLayout) video.view();
            for(int index=0;index<root.getChildCount();index++) {
                TextureView texture=(TextureView) root.getChildAt(index);
                Bitmap bitmap=texture.getBitmap(64,64);
                int colored=0;
                int minX=64,minY=64,maxX=-1,maxY=-1;
                if(bitmap!=null) {
                    for(int y=0;y<64;y++)for(int x=0;x<64;x++){
                        int pixel=bitmap.getPixel(x,y);
                        if((pixel&0x00ffffff)>0x101010)colored++;
                        if(Math.max((pixel>>16)&255,Math.max((pixel>>8)&255,pixel&255))>30){minX=Math.min(minX,x);minY=Math.min(minY,y);maxX=Math.max(maxX,x);maxY=Math.max(maxY,y);}
                    }
                    try(var output=new java.io.FileOutputStream(new File(getFilesDir(),name+"-"+index+".png"))){bitmap.compress(Bitmap.CompressFormat.PNG,100,output);}
                }
                mark("PIXELS "+name+" child="+index+" width="+texture.getWidth()+" height="+texture.getHeight()+" alpha="+texture.getAlpha()+" rootAlpha="+root.getAlpha()+" available="+texture.isAvailable()+" colored="+colored);
                if(index==(enhanced?1:0) && colored<100)fail("presented video is black: "+name);
                if(index==1 && enhanced && colored>=100) {
                    float source=(float)video.playback.output.videoWidth/video.playback.output.videoHeight;
                    float target=(float)texture.getWidth()/texture.getHeight();
                    boolean contain=props.getString("resizeMode").equals("contain");
                    float expectedWidth=contain?Math.min(64,64*source/target):64;
                    float expectedHeight=contain?Math.min(64,64*target/source):64;
                    float left=(64-expectedWidth)/2, top=(64-expectedHeight)/2;
                    mark("BOUNDS "+name+" actual="+minX+","+minY+","+maxX+","+maxY+" expected="+left+","+top+","+(63-left)+","+(63-top));
                    if(Math.abs(minX-left)>3 || Math.abs(minY-top)>3 || Math.abs(maxX-(63-left))>3 || Math.abs(maxY-(63-top))>3)fail("video fit bounds mismatch: "+name);
                    if(contain) {
                        // Synthetic source has a dark clock in its upper-left corner and red below.
                        int upper=0,lower=0;
                        int x0=Math.round(left),x1=Math.round(63-left),y0=Math.round(top),y1=Math.round(63-top);
                        int sampleWidth=Math.max(3,Math.round((x1-x0)*0.23f));
                        for(int y=1;y<=3;y++)for(int x=1;x<=sampleWidth;x++) {
                            upper+=(bitmap.getPixel(x0+x,y0+y)>>16)&255;
                            lower+=(bitmap.getPixel(x0+x,y1-y)>>16)&255;
                        }
                        int rightTop=bitmap.getPixel(x1-1,y0+1),rightBottom=bitmap.getPixel(x1-1,y1-1),leftBottom=bitmap.getPixel(x0+1,y1-1);
                        mark("ORIENTATION_PIXELS "+name+" upper="+upper+" lower="+lower+" corners="+Integer.toHexString(bitmap.getPixel(x0+1,y0+1))+","+Integer.toHexString(rightTop)+","+Integer.toHexString(leftBottom)+","+Integer.toHexString(rightBottom));
                        // The animated diagonal can enter the lower-right corner; its blue
                        // channel stays high, while the upper-right corner remains cyan.
                        if(upper>=lower-sampleWidth*100 || ((leftBottom>>16)&255)<180 || ((leftBottom>>8)&255)>80 || (leftBottom&255)>80 || ((rightTop>>8)&255)<160 || (rightTop&255)<160 || (rightBottom&255)<160)fail("enhanced video corner orientation mismatch: "+name);
                    }
                }
                if(bitmap!=null)bitmap.recycle();
            }
        });
    }
    private void fail(String detail) { if (!completed) { completed=true; Log.e("HGPixelProbe", "FAIL "+detail); } }
    private void update() throws Exception { video.setProp("state", PixelJson.map(props)); }
    private void seek(String token, long position) throws Exception {
        video.command("seek", Map.of("sourceId",props.getJSONObject("source").getString("id"),"requestId",token,"positionMs",position));
    }
    private void run(PixelPlayback.Operation operation) {
        if (completed) return;
        try { operation.run(); } catch (Exception error) { fail(error.toString()); }
    }
    private void event(String name, Object value) {
        run(() -> {
            JSONObject event = new JSONObject((Map<?,?>)value);
            JSONObject data = event.getJSONObject("payload");
            String type = event.getString("type");
            if (!type.equals("progress")) mark(event.toString());
            if (type.equals("enhancementStatus")) {
                JSONObject actual=data.getJSONObject("settings"), wanted=props.getJSONObject("enhancement");
                if(actual.getBoolean("anime4kOn")!=wanted.getBoolean("anime4kOn") || actual.getBoolean("sharpenOn")!=wanted.getBoolean("sharpenOn") || actual.getInt("strength")!=wanted.getInt("strength")) {
                    mark("IGNORED_OLD_SETTINGS");return;
                }
            }
            if (type.equals("error")) { fail(event.toString()); return; }
            if (type.equals("load")) {
                loads++;
                if (Math.abs(data.getLong("durationMs")-30_000)>300) { fail("duration "+data);return; }
                if (step==0) firstPlayer=video.playback.player;
                if (step==20) armReplay();
                if (step==8 && data.getString("sourceId").equals("offline-2")) {
                    step=9;
                    seek("replaced-4",4000);
                    seek("latest-9",9000);
                }
            }
            if (step==0 && type.equals("progress") && data.getLong("positionMs")>=900) {
                step=1; props.put("paused",true); update();
                handler.postDelayed(() -> run(() -> {
                    pausedPosition=video.playback.player.getCurrentPosition();
                    handler.postDelayed(() -> run(() -> {
                        int current=video.playback.player.getCurrentPosition();
                        if (Math.abs(current-pausedPosition)>40) { fail("paused clock advanced");return; }
                        mark("RAW_PAUSED_HOLD "+current); step=2; seek("paused-7",7000);
                        inspect("raw",false);
                    }),400);
                }),250);
            }
            if (step==2 && type.equals("seeked") && data.getString("requestId").equals("paused-7")) {
                if (Math.abs(data.getLong("positionMs")-7000)>250) { fail("seek inaccurate "+data);return; }
                mark("PAUSED_SEEK_CONFIRMED "+data.getLong("positionMs"));
                step=3; props.getJSONObject("enhancement").put("sharpenOn",true); update();
            }
            if (type.equals("enhancementStatus") && data.getString("phase").equals("ready")) {
                if (step==3) {
                    if (video.playback.player.isPlaying() || Math.abs(video.playback.player.getCurrentPosition()-7000)>40) {fail("paused redraw moved playback clock");return;}
                    pausedGpuReady=true; mark("PAUSED_GPU_FRAME_READY clock="+video.playback.player.getCurrentPosition());
                    handler.postDelayed(() -> inspect("paused-gpu",true),250);
                    handler.postDelayed(() -> run(() -> { step=31; props.getJSONObject("enhancement").put("anime4kOn",true); update(); }),600);
                }
                if (step==31) {
                    if (video.playback.player.isPlaying() || Math.abs(video.playback.player.getCurrentPosition()-7000)>40) {fail("Anime4K redraw moved playback clock");return;}
                    animeGpuReady=true; mark("ANIME_GPU_FRAME_READY clock="+video.playback.player.getCurrentPosition());
                    handler.postDelayed(() -> inspect("anime-gpu",true),250);
                    handler.postDelayed(() -> run(this::startPlayingEnhancement),600);
                }
                if (step==4 && !playingGpuReady) {
                    playingGpuReady=true;
                    mark("PLAYING_GPU_FRAME_READY");
                    handler.postDelayed(() -> inspect("playing-gpu",true),250);
                    handler.postDelayed(() -> run(() -> { step=5; seek("enhanced-11",11_000); }),1000);
                }
                if (step==10) {
                    step=11;
                    props.getJSONObject("enhancement").put("strength",10); update();
                    props.getJSONObject("enhancement").put("strength",90); update();
                } else if (step==11 && data.getJSONObject("settings").getInt("strength")==90) {
                    mark("LATEST_STRENGTH_READY 90");
                    handler.postDelayed(() -> inspect("source-2-gpu",true),250);
                    handler.postDelayed(() -> run(() -> {
                        testCover();
                    }),500);
                } else if (step==14) {
                    mark("REACTIVATED_GPU_READY");
                    handler.postDelayed(() -> inspect("reactivated-gpu",true),250);
                    handler.postDelayed(() -> run(() -> {
                        armReplay();
                    }),500);
                }
            }
            if (step==5 && type.equals("seeked") && data.getString("requestId").equals("enhanced-11")) {
                if (Math.abs(data.getLong("positionMs")-11_000)>300) { fail("enhanced seek "+data);return; }
                mark("ENHANCED_SEEK_CONFIRMED");
                props.getJSONObject("enhancement").put("sharpenOn",false); update();
                handler.postDelayed(() -> run(this::checkRawReturn),500);
            }
            if (step==9 && type.equals("seeked") && data.getString("requestId").equals("latest-9")) {
                if (!data.getString("sourceId").equals("offline-2") || Math.abs(data.getLong("positionMs")-9000)>300) {fail("source epoch/latest seek wrong "+data);return;}
                if (!pausedGpuReady || !playingGpuReady || !animeGpuReady) { fail("GPU readiness missing");return; }
                step=10;
                props.put("paused",true); props.getJSONObject("enhancement").put("sharpenOn",true); update();
            }
            if(type.equals("end")) {
                if(step==15){step=16;seek("replay-0",0);}
                else if(step==17){step=18;props.put("paused",true);update();seek("paused-replay-0",0);}
            }
            if(step==16 && type.equals("seeked") && data.getString("requestId").equals("replay-0")) {
                handler.postDelayed(() -> run(() -> {
                    if(video.playback.player!=replayPlayer || !replayPlayer.isPlaying() || replayPlayer.getCurrentPosition()<300){fail("EOF seek did not resume latest playing intent");return;}
                    mark("EOF_REPLAY_ADVANCED "+replayPlayer.getCurrentPosition());step=17;seek("second-eof",29900);
                }),600);
            }
            if(step==18 && type.equals("seeked") && data.getString("requestId").equals("paused-replay-0")) {
                if(video.playback.player!=replayPlayer || replayPlayer.isPlaying() || replayPlayer.getCurrentPosition()>40){fail("EOF seek ignored paused intent");return;}
                // Reproduce a delayed platform snapshot independently of render-worker timing.
                JSONObject stale=new JSONObject(props.toString()).put("paused",false);
                video.playback.apply(stale,video.playback.generation);
                video.playback.reapplyControls();
                int restoredPosition=replayPlayer.getCurrentPosition();
                if(replayPlayer.isPlaying()){fail("completion used stale playing intent");return;}
                mark("LATEST_CONTROLS_REAPPLIED");
                handler.postDelayed(() -> run(() -> {
                    if(video.playback.player!=replayPlayer || replayPlayer.isPlaying() || Math.abs(replayPlayer.getCurrentPosition()-restoredPosition)>40){fail("latest paused intent did not hold");return;}
                    mark("EOF_PAUSED_HOLD "+replayPlayer.getCurrentPosition());completed=true;
                    mark(replayOnly?"PASS replayAfterEnd=true pausedReplay=true samePlayer=true":"PASS pausedGpu=true playingGpu=true animeGpu=true duration=30000 samePlayer=true orientationRestored=true newSourceLatestSeek=9000 latestStrength=90 gpuResize=true activeCycle=true replayAfterEnd=true pausedReplay=true closeWhileGpu=true");
                    handler.postDelayed(this::finish,250);
                }),600);
            }
        });
    }
    private void testCover() throws Exception {
        props.put("resizeMode","cover");update();
        handler.postDelayed(() -> inspect("cover-gpu",true),250);
        handler.postDelayed(() -> run(() -> {
            props.put("resizeMode","contain");update();
            handler.postDelayed(() -> inspect("contain-gpu",true),250);
            handler.postDelayed(() -> run(() -> {step=12;orientation.setProp("request",Map.of("serial",3,"landscape",true));}),600);
        }),600);
    }
    private void armReplay() throws Exception { replayPlayer=video.playback.player;step=15;props.put("paused",false);update();seek("near-eof",29900); }
    private void startPlayingEnhancement() throws Exception {
        if (video.playback.player!=firstPlayer) { fail("second decoder created");return; }
        if (!pausedGpuReady) mark("PAUSED_GPU_NOT_READY");
        props.getJSONObject("enhancement").put("anime4kOn",false).put("sharpenOn",false); update();
        handler.postDelayed(() -> run(() -> {
            step=4; props.put("paused",false); props.getJSONObject("enhancement").put("sharpenOn",true); update();
        }),150);
    }
    private void checkRawReturn() throws Exception {
        if (video.playback.player!=firstPlayer) { fail("fallback changed decoder");return; }
        mark("SAME_PLAYER_RAW_RETURN pos="+video.playback.player.getCurrentPosition()+" loads="+loads);
        step=6;
        orientation.setProp("request",Map.of("serial",1,"landscape",true));
    }
    private void orientationEvent(String name,Object value) {
        run(() -> {
            mark("ORIENTATION "+new JSONObject((Map<?,?>)value));
            Map<?,?> state=(Map<?,?>)value;
            if (state.get("error")!=null) {fail("orientation rejected "+state);return;}
            if (step==6 && Boolean.TRUE.equals(state.get("landscape"))) {
                if (getResources().getConfiguration().orientation!=android.content.res.Configuration.ORIENTATION_LANDSCAPE) {fail("orientation false acknowledgement");return;}
                step=7;handler.postDelayed(() -> orientation.setProp("request",Map.of("serial",2,"landscape",false)),500);
            } else if (step==7 && Boolean.FALSE.equals(state.get("landscape"))) {
                if (getResources().getConfiguration().orientation!=android.content.res.Configuration.ORIENTATION_PORTRAIT) {fail("portrait restore false acknowledgement");return;}
                step=8;
                props.getJSONObject("source").put("id","offline-2");
                update();
            } else if (step==12 && Boolean.TRUE.equals(state.get("landscape"))) {
                handler.postDelayed(() -> inspect("landscape-gpu",true),250);
                handler.postDelayed(() -> run(() -> {step=13;orientation.setProp("request",Map.of("serial",4,"landscape",false));}),600);
            } else if (step==13 && Boolean.FALSE.equals(state.get("landscape"))) {
                handler.postDelayed(() -> inspect("restored-gpu",true),250);
                handler.postDelayed(() -> run(() -> {
                    props.put("active",false);update();
                    handler.postDelayed(() -> run(() -> {
                        step=14;props.put("active",true);update();
                    }),250);
                }),600);
            }
        });
    }
    @Override protected void onDestroy() {
        handler.removeCallbacksAndMessages(null);
        if (orientation!=null) orientation.destroy();
        if (video!=null) video.destroy();
        mark("DESTROYED");
        super.onDestroy();
    }
}
