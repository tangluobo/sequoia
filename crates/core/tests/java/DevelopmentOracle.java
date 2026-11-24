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
import javafx.stage.Stage;
import javafx.embed.swing.SwingFXUtils;
import javax.imageio.ImageIO;

/** Isolated original-source oracle: own project files only, no managed project launch. */
public final class DevelopmentOracle {
    public static void main(String[] args) throws Exception {
        if (args[0].equals("config")) {
            Files.writeString(Path.of(args[1]),new Gson().toJson(DevelopmentConfigManager.load()));
            return;
        }
        CountDownLatch done=new CountDownLatch(1);
        Platform.startup(()->{
            try {
                DevelopmentManagerPane manager=new DevelopmentManagerPane();
                if(args[0].equals("log_rules")) {
                    var f=DevelopmentManagerPane.class.getDeclaredField("runOutputArea");f.setAccessible(true);var pane=f.get(manager);var type=pane.getClass();
                    var style=type.getDeclaredMethod("styleFor",String.class);style.setAccessible(true);var trim=type.getDeclaredMethod("trimUrlEnd",String.class,int.class,int.class);trim.setAccessible(true);
                    var patternField=type.getDeclaredField("URL_PATTERN");patternField.setAccessible(true);var pattern=(java.util.regex.Pattern)patternField.get(null);List<Map<String,Object>> cases=new ArrayList<>();
                    for(String line:List.of("plain line", "[ERROR] broken", "java.lang.Exception", "Caused by: fail", "运行失败", "[WARNING] warning", "warning: detail", "警告", "BUILD SUCCESS", "启动成功", "编译成功", "运行中", "命令：node", "CMD.EXE /c", "JAVA.EXE Main", "MVN.CMD compile", "MVNW.CMD test", "GRADLE.BAT classes", "[MAVEN] start", "[Gradle] start", "[JAVA] run", "[NPM] run", "[DEBUG] ready", "x DEBUG y", "[ERROR] BUILD SUCCESS", "链接https://a.invalid/中文).", "🙂 [WARN] http://127.0.0.1:1234/hello?q=1)!", "HTTP://a.invalid/a\u00a0b,", "wordhttps://no.invalid/skip")) {
                        Map<String,Object> row=new LinkedHashMap<>();row.put("text",line);row.put("style",style.invoke(pane,line));List<Map<String,Object>> links=new ArrayList<>();var match=pattern.matcher(line);
                        while(match.find()){int end=(Integer)trim.invoke(null,line,match.start(),match.end());if(end<=match.start())continue;Map<String,Object> link=new LinkedHashMap<>();link.put("start",line.codePointCount(0,match.start()));link.put("end",line.codePointCount(0,end));link.put("url",line.substring(match.start(),end));links.add(link);}row.put("links",links);cases.add(row);
                    }
                    Files.writeString(Path.of(args[1]),new Gson().toJson(cases));Platform.runLater(()->{Platform.exit();done.countDown();});return;
                }
                if(args[0].equals("diff")) {
                    var parse=DevelopmentManagerPane.class.getDeclaredMethod("parseGitDiffHighlights",String.class);parse.setAccessible(true);
                    List<Map<String,Object>> cases=new ArrayList<>();
                    for(String diff:List.of("@@ -1 +1 @@\n-a\n+b", "@@ -0,0 +1,3 @@\n+a\n+b\n+c", "@@ -4,3 +4,0 @@", "@@ -1,2 +1,4 @@\n@@ -9,0 +12 @@", "@@ -1,4 +1,2 @@", "header\r@@ -3 +3 @@\u0085@@ -5,0 +6 @@", "@@ -0,1 +0,0 @@", "not a hunk")) {
                        Map<String,Object> row=new LinkedHashMap<>();row.put("diff",diff);row.put("changes",parse.invoke(manager,diff));cases.add(row);
                    }
                    Files.writeString(Path.of(args[1]),new Gson().toJson(cases));Platform.exit();done.countDown();return;
                }
                Region sidebar=manager.createSidebarPane();sidebar.setMinWidth(250);sidebar.setPrefWidth(250);sidebar.setMaxWidth(250);
                HBox root=new HBox(sidebar,manager);HBox.setHgrow(manager,Priority.ALWAYS);
                Stage stage=new Stage();stage.setScene(new Scene(root,1100,650));stage.show();
                if (!args[0].equals("empty")) {
                    var directories=DevelopmentManagerPane.class.getDeclaredField("directoryList");directories.setAccessible(true);
                    var list=(java.util.List<?>)directories.get(manager);
                    if (!list.isEmpty()) {
                        var select=DevelopmentManagerPane.class.getDeclaredMethod("selectDirectory",list.get(0).getClass());select.setAccessible(true);select.invoke(manager,list.get(0));
                        if (args[0].equals("editor")) {
                            var open=DevelopmentManagerPane.class.getDeclaredMethod("openCodeFile",java.io.File.class);open.setAccessible(true);open.invoke(manager,Path.of(args[2]).toFile());
                        }
                        var tabsField=DevelopmentManagerPane.class.getDeclaredField("navigationTabPane");tabsField.setAccessible(true);
                        TabPane tabs=(TabPane)tabsField.get(manager);if(args[0].equals("version"))tabs.getSelectionModel().select(1);if(args[0].equals("dependencies"))tabs.getSelectionModel().select(2);if(args[0].equals("logs"))tabs.getSelectionModel().select(3);
                    }
                }
                int[] waiting={0};int[] settled={0};boolean[] warmed={false};
                PauseTransition delay=new PauseTransition(Duration.millis(1800));delay.setOnFinished(event->{
                    if(!args[0].equals("empty")) {
                        try{var f=DevelopmentManagerPane.class.getDeclaredField("runConfigurationCombo");f.setAccessible(true);var combo=(ComboBox<?>)f.get(manager);
                            if(combo.getItems().size()<3){
                                if(waiting[0]++>=60){System.err.println("Owned development oracle readiness timeout");stage.close();Platform.exit();done.countDown();return;}
                                delay.setDuration(Duration.millis(300));delay.playFromStart();return;
                            }
                        }catch(Exception e){e.printStackTrace();stage.close();Platform.exit();done.countDown();return;}
                        if(settled[0]++<2){delay.setDuration(Duration.millis(300));delay.playFromStart();return;}
                    }
                    if(!warmed[0]){root.applyCss();root.layout();root.snapshot(null,null);warmed[0]=true;delay.setDuration(Duration.millis(300));delay.playFromStart();return;}
                    try {
                    root.applyCss();root.layout();ImageIO.write(SwingFXUtils.fromFXImage(root.snapshot(null,null),null),"png",Path.of(args[1]).toFile());
                    List<Map<String,Object>> nodes=new ArrayList<>();collect(root,nodes);Files.writeString(Path.of(args[1]+".layout.json"),new Gson().toJson(nodes));
                } catch(Exception e){e.printStackTrace();}finally{stage.close();Platform.exit();done.countDown();}});delay.play();
            }catch(Exception e){e.printStackTrace();Platform.exit();done.countDown();}
        });
        done.await();
    }
    static void collect(Node node,List<Map<String,Object>> out){
        if(node instanceof Labeled label && label.getText()!=null && (!label.getText().isBlank() || label instanceof ButtonBase)){
            var b=node.localToScene(node.getBoundsInLocal());Map<String,Object> item=new LinkedHashMap<>();item.put("text",label.getText());item.put("x",b.getMinX());item.put("y",b.getMinY());item.put("width",b.getWidth());item.put("height",b.getHeight());item.put("font",label.getFont().getSize());item.put("family",label.getFont().getName());item.put("fill",label.getTextFill().toString());if(label.getTooltip()!=null)item.put("tooltip",label.getTooltip().getText());item.put("alignment",label.getAlignment().toString());item.put("padding",label.getPadding().toString());item.put("style",label.getStyle());var textNode=label.lookup(".text");if(textNode!=null){var tb=textNode.localToScene(textNode.getBoundsInLocal());item.put("textBounds",List.of(tb.getMinX(),tb.getMinY(),tb.getWidth(),tb.getHeight()));}out.add(item);
        }
        if(node instanceof Parent parent)for(Node child:parent.getChildrenUnmodifiable())collect(child,out);
    }
}
