package com.tangluobo.tomato.module.connect.view;
import com.google.gson.Gson;
import javafx.application.Platform;
import javafx.animation.PauseTransition;
import javafx.util.Duration;
import javafx.scene.*;
import javafx.scene.control.*;
import javafx.stage.Stage;
import javafx.embed.swing.SwingFXUtils;
import javax.imageio.ImageIO;
import java.nio.file.*;
import java.util.*;
import java.util.concurrent.CountDownLatch;

/** Runs the actual original game rules and view, in an isolated JavaFX scene. */
public final class ColorTransposeOracle {
    static Object field(Object target,String name)throws Exception{var f=target.getClass().getDeclaredField(name);f.setAccessible(true);return f.get(target);}
    static void call(Object target,String name,Class<?>[] types,Object...args)throws Exception{var m=target.getClass().getDeclaredMethod(name,types);m.setAccessible(true);m.invoke(target,args);}
    static Map<String,Object> snapshot(ColorTransposeGamePane pane)throws Exception{
        Map<String,Object> result=new LinkedHashMap<>();for(String key:List.of("n","columns","moveCount","selectedCol"))result.put(key,field(pane,key));
        result.put("status",((Label)field(pane,"statusLabel")).getText());result.put("style",((Label)field(pane,"statusLabel")).getStyle());
        return new Gson().fromJson(new Gson().toJson(result),Map.class);
    }
    public static void main(String[] args)throws Exception{
        CountDownLatch done=new CountDownLatch(1);Platform.startup(()->{try{
            ColorTransposeGamePane pane=new ColorTransposeGamePane();List<Map<String,Object>> cases=new ArrayList<>();
            if(args[0].equals("cases")){
                for(String dimension:List.of("3","2","20","-7","100","invalid","2147483648"," 4 ")){
                    ((TextField)field(pane,"dimField")).setText(dimension);call(pane,"startGame",new Class<?>[]{});
                    Map<String,Object> row=new LinkedHashMap<>();row.put("dimension",dimension);row.put("start",snapshot(pane));List<Map<String,Object>> states=new ArrayList<>();
                    for(int column:new int[]{0,0,0,1,2,0,2,1,0}){call(pane,"onColumnClick",new Class<?>[]{int.class},column%((Integer)field(pane,"n")));states.add(snapshot(pane));}
                    call(pane,"undo",new Class<?>[]{});row.put("undo",snapshot(pane));row.put("states",states);cases.add(row);
                }
                Files.writeString(Path.of(args[1]),new Gson().toJson(cases));Platform.exit();done.countDown();return;
            }
            Stage stage=new Stage();stage.setScene(new Scene(pane,896,650));stage.show();if(args[0].equals("selected"))call(pane,"onColumnClick",new Class<?>[]{int.class},0);if(args[0].equals("moved")){call(pane,"onColumnClick",new Class<?>[]{int.class},0);call(pane,"onColumnClick",new Class<?>[]{int.class},2);}
            PauseTransition delay=new PauseTransition(Duration.millis(700));delay.setOnFinished(event->{try{pane.applyCss();pane.layout();ImageIO.write(SwingFXUtils.fromFXImage(pane.snapshot(null,null),null),"png",Path.of(args[1]).toFile());Files.writeString(Path.of(args[1]+".state.json"),new Gson().toJson(snapshot(pane)));}catch(Exception e){e.printStackTrace();}finally{stage.close();Platform.exit();done.countDown();}});delay.play();
        }catch(Exception e){e.printStackTrace();Platform.exit();done.countDown();}});done.await();
    }
}
