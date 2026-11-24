import javafx.application.*;
import javafx.animation.PauseTransition;
import javafx.scene.*;
import javafx.scene.control.*;
import javafx.stage.*;
import javafx.util.Duration;
import javafx.embed.swing.SwingFXUtils;
import java.nio.file.*;
import java.util.*;
import java.util.prefs.*;
import javax.imageio.ImageIO;
import com.google.gson.*;
import com.tangluobo.tomato.module.tools.*;
/** Only this owned scene is captured. Preference and home storage are isolated. */
public class ToolPaneOracle {
 public static void main(String[] args){Application.launch(Probe.class,args);}
 public static class MemoryFactory implements PreferencesFactory {
  private final Preferences root=new Memory(null,"");
  public Preferences userRoot(){return root;} public Preferences systemRoot(){return root;}
 }
 public static class Memory extends AbstractPreferences {
  private final Map<String,String> values=new HashMap<>();
  private final Map<String,Memory> children=new HashMap<>();
  Memory(AbstractPreferences parent,String name){super(parent,name);}
  protected void putSpi(String key,String value){values.put(key,value);}
  protected String getSpi(String key){return values.get(key);}
  protected void removeSpi(String key){values.remove(key);}
  protected void removeNodeSpi(){values.clear();children.clear();}
  protected String[] keysSpi(){return values.keySet().toArray(String[]::new);}
  protected String[] childrenNamesSpi(){return children.keySet().toArray(String[]::new);}
  protected AbstractPreferences childSpi(String name){return children.computeIfAbsent(name,n->new Memory(this,n));}
  protected void syncSpi(){} protected void flushSpi(){}
 }
 static void nodes(Node node,JsonArray out){
  var item=new JsonObject();item.addProperty("class",node.getClass().getSimpleName());
  if(node instanceof Labeled label)item.addProperty("text",label.getText());
  else if(node instanceof TextInputControl input)item.addProperty("text",input.getPromptText());
  var b=node.localToScene(node.getBoundsInLocal());
  item.addProperty("x",b.getMinX());item.addProperty("y",b.getMinY());item.addProperty("width",b.getWidth());item.addProperty("height",b.getHeight());
  item.addProperty("visible",node.isVisible());out.add(item);
  if(node instanceof Parent p)for(var child:p.getChildrenUnmodifiable())nodes(child,out);
 }
 public static class Probe extends Application {
  public void start(Stage stage)throws Exception {
   var args=getParameters().getRaw();String tool=args.get(0);int tab=Integer.parseInt(args.get(1));Path path=Path.of(args.get(2));
   Parent pane=switch(tool){case "shortcut"->new DesktopShortcutPane();case "icon"->new ImageBackgroundRemoverPane();case "downloader"->new DownloaderPane();default->throw new IllegalArgumentException(tool);};
   int sceneHeight=Integer.getInteger("tomato.oracle.height",848);
   stage.initStyle(StageStyle.UNDECORATED);stage.setScene(new Scene(pane,895,sceneHeight));stage.setOpacity(.01);stage.show();stage.toBack();pane.applyCss();pane.layout();
   if(tool.equals("shortcut")){var tabs=(TabPane)pane.lookup(".tab-pane");tabs.getSelectionModel().select(tab);}
   if(tool.equals("icon") && tab==1) {
    var type=ImageBackgroundRemoverPane.class;var source=new java.io.File(args.get(3));
    var mode=type.getDeclaredField("isDirMode");mode.setAccessible(true);mode.set(pane,false);
    var current=type.getDeclaredField("currentPreviewFile");current.setAccessible(true);current.set(pane,source);
    var files=type.getDeclaredField("sourceFiles");files.setAccessible(true);files.set(pane,new ArrayList<>(List.of(source)));
    var field=type.getDeclaredField("sourcePathField");field.setAccessible(true);((TextField)field.get(pane)).setText(source.toString());
    var place=type.getDeclaredMethod("placeSaveButtonForMode",boolean.class);place.setAccessible(true);place.invoke(pane,false);
    for(var methodName:List.of("showPreviewSection","updatePreview")){var method=type.getDeclaredMethod(methodName);method.setAccessible(true);method.invoke(pane);}
   }
   if(tool.equals("downloader") && tab>0) {
    var take=new PauseTransition(Duration.millis(650));take.setOnFinished(e->{try {
     for(var window:Window.getWindows())if(window.isShowing() && window!=stage && window.getScene().getRoot() instanceof DialogPane dialog) {
      var scene=window.getScene();scene.getRoot().applyCss();scene.getRoot().layout();ImageIO.write(SwingFXUtils.fromFXImage(scene.snapshot(null),null),"png",path.toFile());
      var data=new JsonObject();data.addProperty("sceneWidth",scene.getWidth());data.addProperty("sceneHeight",scene.getHeight());var list=new JsonArray();nodes(scene.getRoot(),list);data.add("nodes",list);Files.writeString(Path.of(path+".json"),new GsonBuilder().setPrettyPrinting().create().toJson(data));
      ((Button)dialog.lookupButton(ButtonType.CANCEL)).fire();break;
     }
    }catch(Exception error){error.printStackTrace();System.exit(1);}});take.play();
    var method=DownloaderPane.class.getDeclaredMethod(tab==1?"showAddDownloadDialog":"showSettingsDialog");method.setAccessible(true);method.invoke(pane);stage.hide();Platform.exit();return;
   }
   var pause=new PauseTransition(Duration.millis(650));pause.setOnFinished(e->{try{
    pane.applyCss();pane.layout();ImageIO.write(SwingFXUtils.fromFXImage(stage.getScene().snapshot(null),null),"png",path.toFile());
    var data=new JsonObject();data.addProperty("tool",tool);data.addProperty("tab",tab);data.addProperty("sceneWidth",895);data.addProperty("sceneHeight",sceneHeight);data.addProperty("ownedSceneOnly",true);data.addProperty("isolatedPreferences",true);var list=new JsonArray();nodes(pane,list);data.add("nodes",list);
    Files.writeString(Path.of(path+".json"),new GsonBuilder().setPrettyPrinting().create().toJson(data));
    stage.hide();Platform.exit();
   }catch(Exception error){error.printStackTrace();System.exit(1);}});pause.play();
  }
 }
}
