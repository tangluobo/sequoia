package com.tangluobo.tomato.module.tools;

import com.google.gson.Gson;
import java.nio.file.*;
import java.util.*;
import java.util.concurrent.CountDownLatch;
import javafx.application.Platform;
import javafx.animation.PauseTransition;
import javafx.util.Duration;
import javafx.scene.*;
import javafx.scene.control.*;
import javafx.scene.layout.*;
import javafx.stage.*;
import javafx.embed.swing.SwingFXUtils;
import javax.imageio.ImageIO;

/** Runs the original settings dialogs with fake, non-executable SDK entries only. */
public final class DevelopmentSettingsOracle {
    public static void main(String[] args) throws Exception {
        String mode=args[0]; Path output=Path.of(args[1]);
        var config=new Gson().fromJson(Files.readString(Path.of(args[2])),DevelopmentConfigManager.ConfigData.class);
        CountDownLatch done=new CountDownLatch(1);
        Platform.startup(()->{
            Stage owner=new Stage(); owner.setScene(new Scene(new StackPane(),640,480));owner.show();
            Platform.runLater(()->{
                int[] phase={0};int[] attempts={0};
                PauseTransition pulse=new PauseTransition(Duration.millis(300));
                pulse.setOnFinished(event->{
                    try {
                        var window=Window.getWindows().stream().filter(w->w!=owner && w.isShowing() && w.getScene()!=null && w.getScene().getRoot() instanceof DialogPane).findFirst().orElse(null);
                        if(window==null){if(attempts[0]++>30)throw new IllegalStateException("Original settings dialog was not shown");pulse.playFromStart();return;}
                        DialogPane root=(DialogPane)window.getScene().getRoot();root.applyCss();root.layout();
                        if(phase[0]++==0){
                            if(root.getContent() instanceof TabPane tabs){
                                if(mode.startsWith("sdks"))tabs.getSelectionModel().select(1);
                                if(mode.startsWith("builds"))tabs.getSelectionModel().select(2);
                            }
                            if(mode.endsWith("selected")){
                                root.applyCss();root.layout();
                                for(Node node:root.lookupAll(".list-view"))if(node instanceof ListView<?> list && list.isVisible())list.getSelectionModel().select(0);
                            }
                            if(mode.equals("tool-type-change"))for(Node node:root.lookupAll(".combo-box"))if(node instanceof ComboBox<?> combo){@SuppressWarnings("unchecked") var types=(ComboBox<String>)combo;types.setValue("GRADLE");}
                            pulse.playFromStart();return;
                        }
                        if(phase[0]==2){root.snapshot(null,null);pulse.playFromStart();return;}
                        ImageIO.write(SwingFXUtils.fromFXImage(root.snapshot(null,null),null),"png",output.toFile());
                        Map<String,Object> layout=new LinkedHashMap<>();layout.put("mode",mode);layout.put("width",root.getWidth());layout.put("height",root.getHeight());
                        List<Map<String,Object>> labels=new ArrayList<>();DevelopmentOracle.collect(root,labels);layout.put("labels",labels);
                        List<Map<String,Object>> controls=new ArrayList<>();collect(root,controls);layout.put("controls",controls);
                        Files.writeString(Path.of(output+".layout.json"),new Gson().toJson(layout));
                        for(ButtonType type:root.getButtonTypes())if(type.getButtonData()==ButtonBar.ButtonData.CANCEL_CLOSE){((Button)root.lookupButton(type)).fire();return;}
                        window.hide();
                    }catch(Exception e){e.printStackTrace();for(Window w:List.copyOf(Window.getWindows()))if(w!=owner)w.hide();owner.hide();Platform.runLater(()->{Platform.exit();done.countDown();});}
                });
                pulse.play();
                try {
                    if(mode.equals("sdk-add")){
                        var method=DevelopmentProjectSettingsDialog.class.getDeclaredMethod("showAddRuntimeDialog",Window.class);method.setAccessible(true);method.invoke(null,owner);
                    }else if(mode.startsWith("tool-")){
                        var method=DevelopmentProjectSettingsDialog.class.getDeclaredMethod("showBuildToolDialog",Window.class,DevelopmentConfigManager.BuildToolEntry.class);method.setAccessible(true);
                        var existing=mode.equals("tool-add")?null:config.getBuildTools().get(mode.equals("tool-gradle")?1:0);method.invoke(null,owner,existing);
                    }else{
                        DevelopmentProjectSettingsDialog.show(owner,"示例项目","MAVEN_NODE","jdk-1","node-1","flutter-1","maven-1",config);
                    }
                }catch(Exception e){e.printStackTrace();}finally{pulse.stop();owner.close();Platform.runLater(()->{Platform.exit();done.countDown();});}
            });
        });
        done.await();
    }
    static void collect(Node node,List<Map<String,Object>> out){
        if(node instanceof Control control){
            var b=control.localToScene(control.getBoundsInLocal());var item=new LinkedHashMap<String,Object>();
            item.put("class",control.getClass().getSimpleName());item.put("styleClasses",control.getStyleClass());item.put("x",b.getMinX());item.put("y",b.getMinY());item.put("width",b.getWidth());item.put("height",b.getHeight());item.put("disabled",control.isDisabled());item.put("visible",control.isVisible());
            if(control instanceof TextInputControl text){item.put("text",text.getText());item.put("prompt",text.getPromptText());item.put("editable",text.isEditable());}
            if(control instanceof ComboBox<?> combo){item.put("value",combo.getValue()==null?null:combo.getValue().toString());item.put("prompt",combo.getPromptText());}
            if(control instanceof ListView<?> list){item.put("selectedIndex",list.getSelectionModel().getSelectedIndex());item.put("items",list.getItems().size());}
            out.add(item);
        }
        if(node instanceof Parent parent)for(Node child:parent.getChildrenUnmodifiable())collect(child,out);
    }
}
